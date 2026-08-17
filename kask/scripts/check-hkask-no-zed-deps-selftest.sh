#!/usr/bin/env bash
# Self-test for the hkask-no-zed-deps gate (check-hkask-no-zed-deps.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" — applied to check-hkask-no-zed-deps.sh per CI
# gate sweep follow-up issue #6. The gate's "0 = 0" failure mode (zero hKask
# manifests found → zero violations → trivially passes) was flagged in the
# sweep. This self-test pins both failure modes:
#   1. Violation: an hKask Cargo.toml with a `[dependencies]` section naming
#      a zed-kask-only crate (e.g. `gpui`).
#   2. Empty: a MANIFEST_PATHS matching zero manifests must exit 0 (no
#      violations) without crashing — pinning the silent-disconnection path.
#
# Design: temp crate dirs are populated with minimal Cargo.toml files, and
# the gate is invoked with MANIFEST_PATHS pointed at the temp globs. Each
# case asserts the gate exits with the expected code and keyword.
#
# Exit codes:
#   0 — all cases behaved as expected (gate is alive)
#   1 — at least one case did not behave as expected (gate is vacuous or broken)
#
# Usage: bash scripts/check-hkask-no-zed-deps-selftest.sh

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-hkask-no-zed-deps.sh"

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
# Case 1: violation — an hKask Cargo.toml depending on a zed-kask crate
# (`gpui`). The gate must exit 1 with "VIOLATION".
#
# The gate runs `find $MANIFEST_PATHS` from the kask root (it cd's there
# internally), so MANIFEST_PATHS must be paths the kask-root cwd can resolve.
# We use absolute paths for the temp tree.
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/crates/hkask-selftest-bad"
cat > "$TMPDIR/crates/hkask-selftest-bad/Cargo.toml" <<'EOF'
[package]
name = "hkask-selftest-bad"
version = "0.0.0"

[dependencies]
gpui = "1.0"
EOF

set +e
CASE1_OUT=$(MANIFEST_PATHS="$TMPDIR/crates/hkask-*" bash "$GATE" 2>&1)
CASE1_RC=$?
set -e

if [ "$CASE1_RC" -ne 1 ]; then
  echo "FAIL (case 1 — violation): expected exit 1, got $CASE1_RC"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE1_OUT" | grep -q "VIOLATION"; then
  echo "FAIL (case 1 — violation): exit 1 but 'VIOLATION' not in output"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 1 — violation): gate detected the zed-kask dependency"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 2: empty MANIFEST_PATHS — zero matching manifests must exit 0 (no
# violations) without crashing. This pins the silent-disconnection path:
# 0 manifests = 0 violations is the correct verdict here, NOT a vacuous pass.
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/empty"

set +e
CASE2_OUT=$(MANIFEST_PATHS="$TMPDIR/empty/*" bash "$GATE" 2>&1)
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
  echo "OK (case 2 — empty): gate exited 0 on empty manifest paths"
fi

if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: hkask-no-zed-deps gate is alive (violation + empty both pinned)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
  echo "The gate is vacuous or broken — see kask/scripts/check-hkask-no-zed-deps.sh."
  exit 1
fi
