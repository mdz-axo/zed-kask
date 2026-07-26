#!/usr/bin/env bash
# CI gate: assert kask/scripts/build/mcp-servers.txt matches the runtime
# canonical registry BUILTIN_SERVERS in
# kask/crates/hkask-mcp-server/src/hkask_mcp_server.rs.
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
KASK_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIST_FILE="$KASK_ROOT/scripts/build/mcp-servers.txt"
REGISTRY_FILE="$KASK_ROOT/crates/hkask-mcp-server/src/hkask_mcp_server.rs"

if [ ! -f "$LIST_FILE" ]; then
    echo -e "${RED}[ERROR]${NC} MCP server list not found: $LIST_FILE"
    exit 1
fi
if [ ! -f "$REGISTRY_FILE" ]; then
    echo -e "${RED}[ERROR]${NC} Runtime registry not found: $REGISTRY_FILE"
    exit 1
fi

# Extract binary names from mcp-servers.txt (skip comments and blank lines).
list_names=$(grep -vE '^\s*#|^\s*$' "$LIST_FILE" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' | sort -u)

# Extract binary names from BUILTIN_SERVERS entries in hkask_mcp_server.rs.
# Each entry looks like: ("short_name", "hkask-mcp-binary-name"),
# We capture the second field of each tuple.
registry_names=$(grep -oE '\(\s*"[^"]+"\s*,\s*"(hkask-mcp-[a-z-]+)"\s*\)' "$REGISTRY_FILE" \
    | sed -E 's/.*,\s*"(hkask-mcp-[a-z-]+)".*/\1/' \
    | sort -u)

if [ -z "$list_names" ] || [ -z "$registry_names" ]; then
    echo -e "${RED}[ERROR]${NC} One or both lists are empty."
    echo "  mcp-servers.txt:"
    sed 's/^/    /' <<< "$list_names"
    echo "  BUILTIN_SERVERS:"
    sed 's/^/    /' <<< "$registry_names"
    exit 1
fi

# Diff the two. `diff <(a) <(b)` returns 0 if identical.
if diff_output=$(diff <(echo "$list_names") <(echo "$registry_names")); then
    count=$(echo "$list_names" | wc -l)
    echo -e "${GREEN}[OK]${NC} mcp-servers.txt matches BUILTIN_SERVERS ($count servers)"
    exit 0
else
    echo -e "${RED}[ERROR]${NC} Drift between mcp-servers.txt and BUILTIN_SERVERS:"
    sed 's/^/    /' <<< "$diff_output"
    echo ""
    echo "Fix: edit $LIST_FILE to match BUILTIN_SERVERS in $REGISTRY_FILE"
    exit 1
fi
