#!/usr/bin/env bash
# Self-test for the mcp-servers drift gate (check-mcp-servers.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" — applied to check-mcp-servers.sh per CI gate
# sweep follow-up issue #6. The gate's "0 = 0" failure mode (both the
# mcp-servers.txt list and the BUILT_IN_MCP_SERVERS registry being empty
# trivially diff as equal) was flagged in the sweep. The gate already has an
# empty-list guard (lines 44-51), but this self-test pins both failure modes
# the gate is supposed to catch:
#   1. Drift: a server in mcp-servers.txt but NOT in the registry (or vice
#      versa) — the canonical silent-drift case the gate exists for.
#   2. Empty: both lists empty — the guard must fire rather than trivially
#      diff as equal.
#
# Design: temp LIST_FILE, REGISTRY_FILE, and OLD_REGISTRY_FILE are populated
# with minimal content, and the gate is invoked with the env vars pointed at
# the temp files. Each case injects one violation class and asserts the gate
# exits 1 with the expected keyword.
#
# Exit codes:
#   0 — all synthetic violations were detected (gate is alive)
#   1 — at least one synthetic violation was NOT detected (gate is vacuous)
#
# Usage: bash scripts/check-mcp-servers-selftest.sh

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-mcp-servers.sh"

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
# Case 1: drift — a server in mcp-servers.txt but NOT in the registry.
# The gate must exit 1 with "Drift".
# ──────────────────────────────────────────────────────────────────────────
cat > "$TMPDIR/mcp-servers.txt" <<'EOF'
# selftest synthetic server list
hkask-mcp-selftest-only-in-list
EOF

cat > "$TMPDIR/mcp_servers.rs" <<'EOF'
// selftest: registry contains a DIFFERENT server than the list.
pub static BUILT_IN_MCP_SERVERS: &[BuiltinMcpServer] = &[
    BuiltinMcpServer { id: "selftest-only-in-registry", binary: "hkask-mcp-selftest-only-in-registry" },
];
EOF

# An old-registry file with no BUILTIN_SERVERS — must not trip the guard.
cat > "$TMPDIR/hkask_mcp_server.rs" <<'EOF'
// selftest: no BUILTIN_SERVERS here
EOF

set +e
CASE1_OUT=$(LIST_FILE="$TMPDIR/mcp-servers.txt" \
  REGISTRY_FILE="$TMPDIR/mcp_servers.rs" \
  OLD_REGISTRY_FILE="$TMPDIR/hkask_mcp_server.rs" \
  bash "$GATE" 2>&1)
CASE1_RC=$?
set -e

if [ "$CASE1_RC" -ne 1 ]; then
  echo "FAIL (case 1 — drift): expected exit 1, got $CASE1_RC"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE1_OUT" | grep -q "Drift"; then
  echo "FAIL (case 1 — drift): exit 1 but 'Drift' not in output"
  printf '%s\n' "$CASE1_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 1 — drift): gate detected list/registry drift"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 2: empty-list guard — both lists empty must exit 1 with "empty".
# This pins the silent-disconnection guard: 0 = 0 must NOT trivially pass.
# ──────────────────────────────────────────────────────────────────────────
cat > "$TMPDIR/mcp-servers-empty.txt" <<'EOF'
# only comments and blank lines

# selftest: no actual server entries
EOF

cat > "$TMPDIR/mcp_servers-empty.rs" <<'EOF'
// selftest: no BUILT_IN_MCP_SERVERS entries with a binary: field
pub static BUILT_IN_MCP_SERVERS: &[BuiltinMcpServer] = &[];
EOF

set +e
CASE2_OUT=$(LIST_FILE="$TMPDIR/mcp-servers-empty.txt" \
  REGISTRY_FILE="$TMPDIR/mcp_servers-empty.rs" \
  OLD_REGISTRY_FILE="$TMPDIR/hkask_mcp_server.rs" \
  bash "$GATE" 2>&1)
CASE2_RC=$?
set -e

if [ "$CASE2_RC" -ne 1 ]; then
  echo "FAIL (case 2 — empty guard): expected exit 1, got $CASE2_RC"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$CASE2_OUT" | grep -q "empty"; then
  echo "FAIL (case 2 — empty guard): exit 1 but 'empty' not in output"
  printf '%s\n' "$CASE2_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 2 — empty guard): guard fired on zero-entry lists"
fi

if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: mcp-servers gate is alive (drift + empty-guard both detected)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not detect their synthetic violation"
  echo "The gate is vacuous — see kask/scripts/check-mcp-servers.sh."
  exit 1
fi
