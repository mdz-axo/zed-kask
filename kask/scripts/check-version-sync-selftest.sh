#!/usr/bin/env bash
# Self-test for the version-sync gate (check-version-sync.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" — applied to check-version-sync.sh per CI gate
# sweep follow-up issue #6. The gate's "0 = 0" failure mode (zero manifests in
# MANIFEST_DIR → zero drift found → trivially passes) was flagged in the sweep.
# This self-test pins both failure modes:
#   1. Drift: a manifest whose `version:` field does NOT match the workspace
#      version — the canonical drift case the gate exists for.
#   2. Empty: a MANIFEST_DIR with zero manifests must still exit 0 (no drift
#      to find) without crashing — pinning the silent-disconnection path so a
#      future change can't invert it (e.g. exit 1 on empty, hiding a real
#      disconnection behind a different failure mode).
#
# Design: temp CARGO_TOML, MANIFEST_DIR, MCP_SERVERS_DIR, and KASK_ROOT are
# populated with minimal content, and the gate is invoked with the env vars
# pointed at the temp tree. Each case asserts the gate exits with the expected
# code and keyword.
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

# A temp kask root containing only a README whose **Version:** matches, so the
# README scan (step 5) does not produce a false positive in the drift case.
mkdir -p "$TMPDIR/kask"
cat > "$TMPDIR/kask/README.md" <<EOF
# selftest kask
**Version:** v$WORKSPACE_VERSION
EOF

# An empty mcp-servers dir so the provenance-literal scan (step 4) finds nothing.
mkdir -p "$TMPDIR/mcp-servers"

# ──────────────────────────────────────────────────────────────────────────
# Case 1: drift — a manifest whose `version:` field does NOT match the
# workspace version. The gate must exit 1 with "DRIFT".
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/manifests-drift"
cat > "$TMPDIR/manifests-drift/selftest.yaml" <<EOF
manifest:
  id: selftest
  version: "0.0.0-wrong"
ledger:
  span_namespace: reg.skill.selftest
EOF

set +e
CASE1_OUT=$(CARGO_TOML="$TMPDIR/Cargo.toml" \
  MANIFEST_DIR="$TMPDIR/manifests-drift" \
  MCP_SERVERS_DIR="$TMPDIR/mcp-servers" \
  KASK_ROOT="$TMPDIR/kask" \
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
  echo "OK (case 1 — drift): gate detected the manifest version drift"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 2: empty MANIFEST_DIR — zero manifests must exit 0 (no drift to find)
# without crashing. This pins the silent-disconnection path: 0 manifests = 0
# drift is the correct verdict here, NOT a vacuous pass. The gate must not
# crash or invert to exit 1.
# ──────────────────────────────────────────────────────────────────────────
mkdir -p "$TMPDIR/manifests-empty"

set +e
CASE2_OUT=$(CARGO_TOML="$TMPDIR/Cargo.toml" \
  MANIFEST_DIR="$TMPDIR/manifests-empty" \
  MCP_SERVERS_DIR="$TMPDIR/mcp-servers" \
  KASK_ROOT="$TMPDIR/kask" \
  bash "$GATE" 2>&1)
CASE2_RC=$?
set -e

if [ "$CASE2_RC" -ne 0 ]; then
  echo "FAIL (case 2 — empty): expected exit 0, got $CASE2_RC"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE2_OUT" | grep -q "in sync"; then
  echo "FAIL (case 2 — empty): exit 0 but 'in sync' not in output"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 2 — empty): gate exited 0 on empty manifest dir"
fi

if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: version-sync gate is alive (drift detected, empty path pinned)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
  echo "The gate is vacuous or broken — see kask/scripts/check-version-sync.sh."
  exit 1
fi
