#!/usr/bin/env bash
# Self-test for the version-sync gate (check-version-sync.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" — applied to check-version-sync.sh per CI gate
# sweep follow-up issue #6. The gate's "0 = 0" failure mode (zero drift
# candidates → zero drift found → trivially passes) was flagged in the sweep.
# This self-test pins both failure modes:
#   1. Drift: a README whose `**Version:` line does NOT match the workspace
#      version — the canonical drift case the gate exists for.
#   2. Clean: a tree with no drift must exit 0 ("in sync") without crashing —
#      pinning the trivial-pass path so a future change can't invert it (e.g.
#      exit 1 on empty, hiding a real disconnection behind a different failure
#      mode).
#
# History: this self-test previously pinned manifest-dir drift and an empty
# manifest dir. The manifest registry was removed in 5f4cf5f10d, so the gate's
# manifest checks were deleted; the cases were re-based onto the two live
# checks (MCP provenance literals + README version lines).
#
# Design: temp CARGO_TOML, MCP_SERVERS_DIR, and KASK_ROOT are populated with
# minimal content, and the gate is invoked with the env vars pointed at the
# temp tree. Each case asserts the gate exits with the expected code and
# keyword.
#
# Exit codes:
#   0 — all cases behaved as expected (gate is alive)
#   1 — at least one case did not behave as expected (gate is vacuous or broken)
#
# Usage: bash scripts/check-version-sync-selftest.sh

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-version-sync.sh"

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

# A temp workspace version (1.2.3) declared in a temp root Cargo.toml. The
# gate parses [workspace.package] version from this file.
WORKSPACE_VERSION="1.2.3"
cat > "$TMPDIR/Cargo.toml" <<EOF
[workspace]
members = []

[workspace.package]
version = "$WORKSPACE_VERSION"
edition = "2021"
EOF

# An empty mcp-servers dir so the provenance-literal scan (step 1) finds nothing.
# Both cases share this; drift is introduced via the README tree only.
mkdir -p "$TMPDIR/mcp-servers"

# ──────────────────────────────────────────────────────────────────────────
# Case 1: drift — a README whose `**Version:**` line does NOT match the
# workspace version. The gate must exit 1 with "DRIFT".
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/kask-drift"
cat > "$TMPDIR/kask-drift/README.md" <<EOF
# selftest kask (drift)
**Version:** v0.0.0-wrong
EOF

set +e
CASE1_OUT=$(CARGO_TOML="$TMPDIR/Cargo.toml" \
  MCP_SERVERS_DIR="$TMPDIR/mcp-servers" \
  KASK_ROOT="$TMPDIR/kask-drift" \
  bash "$GATE" 2>&1)
CASE1_RC=$?
set -e

if [ "$CASE1_RC" -ne 1 ]; then
  echo "FAIL (case 1 — drift): expected exit 1, got $CASE1_RC"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE1_OUT" | grep -q "DRIFT"; then
  echo "FAIL (case 1 — drift): exit 1 but 'DRIFT' not in output"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 1 — drift): gate detected the README version drift"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 2: clean tree — a README whose `**Version:**` matches the workspace
# version and no MCP provenance literals. The gate must exit 0 ("in sync")
# without crashing. This pins the trivial-pass path: zero drift = exit 0 is
# the correct verdict here, NOT a vacuous pass, and the gate must not invert
# to exit 1.
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/kask-clean"
cat > "$TMPDIR/kask-clean/README.md" <<EOF
# selftest kask (clean)
**Version:** v$WORKSPACE_VERSION
EOF

set +e
CASE2_OUT=$(CARGO_TOML="$TMPDIR/Cargo.toml" \
  MCP_SERVERS_DIR="$TMPDIR/mcp-servers" \
  KASK_ROOT="$TMPDIR/kask-clean" \
  bash "$GATE" 2>&1)
CASE2_RC=$?
set -e

if [ "$CASE2_RC" -ne 0 ]; then
  echo "FAIL (case 2 — clean): expected exit 0, got $CASE2_RC"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE2_OUT" | grep -q "in sync"; then
  echo "FAIL (case 2 — clean): exit 0 but 'in sync' not in output"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 2 — clean): gate exited 0 on a clean tree"
fi

if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: version-sync gate is alive (drift detected, clean path pinned)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
  echo "The gate is vacuous or broken — see kask/scripts/check-version-sync.sh."
  exit 1
fi