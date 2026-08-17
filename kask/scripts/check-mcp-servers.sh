#!/usr/bin/env bash
# CI gate: assert kask/scripts/build/mcp-servers.txt matches the runtime
# canonical registry BUILT_IN_MCP_SERVERS in
# kask/crates/kask_bridge/src/mcp_servers.rs.
#
# Drift between the install/release surface and the runtime registry causes
# partial installs (missing MCP server binaries) with no compile-time error.
# This check fails CI before such drift can ship.
#
# Usage: cd kask && bash scripts/check-mcp-servers.sh
# Exit codes: 0 = match, 1 = drift detected

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

# Resolve paths relative to the kask/ workspace (script lives at kask/scripts/).
# Each path is overridable via env var so the self-test can point at a temp
# tree; the defaults preserve the production behavior exactly.
KASK_ROOT="${KASK_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
LIST_FILE="${LIST_FILE:-$KASK_ROOT/scripts/build/mcp-servers.txt}"
REGISTRY_FILE="${REGISTRY_FILE:-$KASK_ROOT/crates/kask_bridge/src/mcp_servers.rs}"

if [ ! -f "$LIST_FILE" ]; then
    echo -e "${RED}[ERROR]${NC} MCP server list not found: $LIST_FILE"
    exit 1
fi
if [ ! -f "$REGISTRY_FILE" ]; then
    echo -e "${RED}[ERROR]${NC} Runtime registry not found: $REGISTRY_FILE"
    exit 1
fi

# Extract binary names from mcp-servers.txt (skip comments and blank lines).
# `|| true` because grep exits 1 on zero matches, which under `pipefail`
# would abort before the empty-list guard below can fire — that guard is
# the whole point of distinguishing "0 = 0 trivially equal" from real drift.
list_names=$(grep -vE '^\s*#|^\s*$' "$LIST_FILE" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' | sort -u || true)

# Extract binary names from BUILT_IN_MCP_SERVERS entries in mcp_servers.rs.
# Each entry is a BuiltinMcpServer struct literal with a `binary:` field:
#     BuiltinMcpServer { id: "codegraph", binary: "hkask-mcp-codegraph", ... }
# We capture the value of the `binary:` field.
# `|| true` for the same reason as above — an empty registry must reach the
# empty-list guard, not abort the pipeline.
registry_names=$(grep -oE 'binary:\s*"(hkask-mcp-[a-z0-9_-]+)"' "$REGISTRY_FILE" \
    | sed -E 's/.*"(hkask-mcp-[a-z0-9_-]+)".*/\1/' \
    | sort -u || true)

if [ -z "$list_names" ] || [ -z "$registry_names" ]; then
    echo -e "${RED}[ERROR]${NC} One or both lists are empty."
    echo "  mcp-servers.txt:"
    sed 's/^/    /' <<< "$list_names"
    echo "  BUILT_IN_MCP_SERVERS:"
    sed 's/^/    /' <<< "$registry_names"
    exit 1
fi

# Diff the two. `diff <(a) <(b)` returns 0 if identical.
if diff_output=$(diff <(echo "$list_names") <(echo "$registry_names")); then
    count=$(echo "$list_names" | wc -l)
    echo -e "${GREEN}[OK]${NC} mcp-servers.txt matches BUILT_IN_MCP_SERVERS ($count servers)"
else
    echo -e "${RED}[ERROR]${NC} Drift between mcp-servers.txt and BUILT_IN_MCP_SERVERS:"
    sed 's/^/    /' <<< "$diff_output"
    echo ""
    echo "Fix: edit $LIST_FILE to match BUILT_IN_MCP_SERVERS in $REGISTRY_FILE"
    exit 1
fi

# Guard: BUILTIN_SERVERS must not be re-introduced in its old location.
# The canonical registry was consolidated into kask_bridge::BUILT_IN_MCP_SERVERS
# (PR #127, "Remove duplicate MCP server registry"). A parallel list in
# hkask_mcp_server.rs would reintroduce the silent drift this check exists to
# prevent — the previous duplicate used id "kanban" while the canonical list
# uses "kata-kanban". See the "Do NOT re-introduce" note in that file.
OLD_REGISTRY_FILE="${OLD_REGISTRY_FILE:-$KASK_ROOT/crates/hkask-mcp-server/src/hkask_mcp_server.rs}"
if grep -nE '^[[:space:]]*(pub[[:space:]]+)?(const|static)[[:space:]]+BUILTIN_SERVERS\b' "$OLD_REGISTRY_FILE" >/dev/null; then
    echo -e "${RED}[ERROR]${NC} BUILTIN_SERVERS was re-introduced in $OLD_REGISTRY_FILE"
    echo "  The canonical registry is BUILT_IN_MCP_SERVERS in $REGISTRY_FILE."
    echo "  Remove the parallel list from $OLD_REGISTRY_FILE."
    exit 1
fi
echo -e "${GREEN}[OK]${NC} BUILTIN_SERVERS not re-introduced in old location"
exit 0
