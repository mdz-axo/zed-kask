#!/usr/bin/env bash
# Self-test for the mcp-servers drift gate (check-mcp-servers.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" — applied to check-mcp-servers.sh per CI gate
# sweep follow-up issue #6. The gate's "0 = 0" failure mode (both the
# mcp-servers.txt list and the BUILT_IN_MCP_SERVERS registry being empty
# trivially diff as equal) was flagged in the sweep. The gate already has an
# empty-list guard (lines 44-51), but this self-test pins the failure modes
# the gate is supposed to catch:
#   1. Drift: a server in mcp-servers.txt but NOT in the registry (or vice
#      versa) — the canonical silent-drift case the gate exists for.
#   2. Empty: both lists empty — the guard must fire rather than trivially
#      diff as equal.
#   3. Disk drift (unregistered): a server crate on disk appearing in neither
#      the list nor the registry. Comparing list vs registry proves only that
#      those two agree with each other; the tree itself was never consulted.
#   4. Disk drift (residue): a server removed from list and registry whose
#      crate directory was left behind — the realistic direction, since a
#      capability removal touches many files at once.
#   5. Disk empty: zero manifest-bearing server crates must fail, not pass
#      trivially — the same 0 = 0 hazard as case 2, on the third set.
#   6. Clean control: all three sets agreeing must still exit 0.
#   7. Residue tolerance: a directory with no Cargo.toml is not a crate and
#      must not trip the gate.
#
# Design: temp LIST_FILE, REGISTRY_FILE, OLD_REGISTRY_FILE and SERVERS_DIR are
# populated
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


# ──────────────────────────────────────────────────────────────────────────
# Disk-reconciliation cases (3-7). The list and registry legs AGREE in every
# case below, so any failure here is attributable to the disk leg alone.
#
# The disk leg closes a scope gap: list-vs-registry proves only that those two
# agree with each other and says nothing about the tree. A server crate wired
# into neither — or one left behind as residue after a capability removal —
# passed silently. Three sets must agree, not two.
# ──────────────────────────────────────────────────────────────────────────
AGREED_LIST="$TMPDIR/agreed-list.txt"
AGREED_REG="$TMPDIR/agreed-registry.rs"
printf '%s\n' "hkask-mcp-alpha" > "$AGREED_LIST"
{
  printf 'pub static BUILT_IN_MCP_SERVERS: &[BuiltinMcpServer] = &[\n'
  printf '    BuiltinMcpServer { id: "alpha", binary: "hkask-mcp-alpha" },\n'
  printf '];\n'
} > "$AGREED_REG"

# run_disk_case <label> <expected_rc> [crate-dir...]
# Builds a SERVERS_DIR holding one manifest-bearing crate per named dir.
run_disk_case() {
  local label="$1" expected="$2"; shift 2
  local sd d rc out
  sd="$(mktemp -d "$TMPDIR/servers-XXXXXX")"
  for d in "$@"; do
    mkdir -p "$sd/$d/src"
    printf '[package]\nname = "%s"\n' "$d" > "$sd/$d/Cargo.toml"
  done
  set +e
  out=$(LIST_FILE="$AGREED_LIST" REGISTRY_FILE="$AGREED_REG" \
    OLD_REGISTRY_FILE="$TMPDIR/hkask_mcp_server.rs" \
    SERVERS_DIR="$sd" bash "$GATE" 2>&1)
  rc=$?
  set -e
  if [ "$rc" -ne "$expected" ]; then
    echo "FAIL ($label): expected exit $expected, got $rc"
    printf '%s\n' "$out" | sed 's/^/    /'
    failures=$((failures + 1))
  else
    echo "OK ($label): exit $rc as expected"
  fi
}

# Case 3: an extra server crate on disk, registered nowhere.
run_disk_case "case 3 — unregistered crate on disk" 1 hkask-mcp-alpha hkask-mcp-ghost

# Case 4: list and registry name a server whose crate directory is gone.
run_disk_case "case 4 — registered server missing from disk" 1 hkask-mcp-other

# Case 5: zero manifest-bearing crates must fail, not pass trivially.
run_disk_case "case 5 — zero server crates on disk" 1

# Case 6: all three sets agree.
run_disk_case "case 6 — clean three-way match" 0 hkask-mcp-alpha

# Case 7: a directory with no Cargo.toml is residue, not a server crate, and
# must not trip the gate — otherwise an already-correct cleanup fails CI.
RESIDUE_DIR="$TMPDIR/servers-residue"
mkdir -p "$RESIDUE_DIR/hkask-mcp-alpha/src" "$RESIDUE_DIR/hkask-mcp-leftover"
printf '[package]\nname = "hkask-mcp-alpha"\n' > "$RESIDUE_DIR/hkask-mcp-alpha/Cargo.toml"
set +e
CASE7_OUT=$(LIST_FILE="$AGREED_LIST" REGISTRY_FILE="$AGREED_REG" \
  OLD_REGISTRY_FILE="$TMPDIR/hkask_mcp_server.rs" \
  SERVERS_DIR="$RESIDUE_DIR" bash "$GATE" 2>&1)
CASE7_RC=$?
set -e
if [ "$CASE7_RC" -ne 0 ]; then
  echo "FAIL (case 7 — manifest-less residue dir): expected exit 0, got $CASE7_RC"
  printf '%s\n' "$CASE7_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 7 — manifest-less residue dir): gate counted crates, not bare dirs"
fi
if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: mcp-servers gate is alive (list/registry drift, empty guard, and all disk-reconciliation directions detected)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not detect their synthetic violation"
  echo "The gate is vacuous — see kask/scripts/check-mcp-servers.sh."
  exit 1
fi
