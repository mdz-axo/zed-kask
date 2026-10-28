#!/usr/bin/env bash
# Self-test for the mcp-tool-tests gate (check-mcp-tool-tests.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" — applied to check-mcp-tool-tests.sh per CI gate
# sweep follow-up issue #6. The gate's "0 = 0" failure mode (zero MCP servers
# discovered → zero violations → trivially passes) was flagged in the sweep.
# This self-test pins both failure modes:
#   1. Violation: a server with NO tests dir and NOT in the allowlist must
#      fail with "no tool-behavior contract tests".
#   2. Empty: a SCAN_DIRS matching zero servers must exit 0 (no violations)
#      without crashing — pinning the silent-disconnection path.
#
# Design: temp server dirs are populated with minimal Cargo.toml files (no
# tests dir), and the gate is invoked with SCAN_DIRS pointed at the temp glob
# and ALLOWLIST set to empty (so the synthetic server is not allowlisted).
# Each case asserts the gate exits with the expected code and keyword.
#
# Exit codes:
#   0 — all cases behaved as expected (gate is alive)
#   1 — at least one case did not behave as expected (gate is vacuous or broken)
#
# Usage: bash scripts/check-mcp-tool-tests-selftest.sh

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-mcp-tool-tests.sh"

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
# Case 1: violation — a server with NO tests dir and NOT in the allowlist.
# The gate must exit 1 with "no tool-behavior contract tests".
#
# The gate runs `for server_dir in $SCAN_DIRS` from the kask root (it cd's
# there internally), so SCAN_DIRS must be a glob the kask-root cwd can
# resolve. We use an absolute path.
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/hkask-mcp-selftest-bad"
cat > "$TMPDIR/hkask-mcp-selftest-bad/Cargo.toml" <<'EOF'
[package]
name = "hkask-mcp-selftest-bad"
version = "0.0.0"
EOF

set +e
CASE1_OUT=$(SCAN_DIRS="$TMPDIR/hkask-mcp-*" ALLOWLIST="" bash "$GATE" 2>&1)
CASE1_RC=$?
set -e

if [ "$CASE1_RC" -ne 1 ]; then
  echo "FAIL (case 1 — violation): expected exit 1, got $CASE1_RC"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE1_OUT" | grep -q "no tool-behavior contract tests"; then
  echo "FAIL (case 1 — violation): exit 1 but 'no tool-behavior contract tests' not in output"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 1 — violation): gate detected the missing tool-behavior tests"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 2: empty SCAN_DIRS — zero matching servers must exit 0 (no violations)
# without crashing. This pins the silent-disconnection path: 0 servers = 0
# violations is the correct verdict here, NOT a vacuous pass.
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/empty"

set +e
CASE2_OUT=$(SCAN_DIRS="$TMPDIR/empty/*" ALLOWLIST="" bash "$GATE" 2>&1)
CASE2_RC=$?
set -e

if [ "$CASE2_RC" -ne 0 ]; then
  echo "FAIL (case 2 — empty): expected exit 0, got $CASE2_RC"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE2_OUT" | grep -q "summary"; then
  echo "FAIL (case 2 — empty): exit 0 but 'summary' not in output"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 2 — empty): gate exited 0 on empty scan dirs"
fi

if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: mcp-tool-tests gate is alive (violation + empty both pinned)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
  echo "The gate is vacuous or broken — see kask/scripts/check-mcp-tool-tests.sh."
  exit 1
fi
