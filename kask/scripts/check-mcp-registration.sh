#!/usr/bin/env bash
# check-mcp-registration.sh — registration-pin gate (mcp-tool-review Phase 2).
#
# Every server in kask/scripts/build/mcp-servers.txt must carry the
# tool_surface_pin! macro invocation in its src/ — the pin that makes a
# silent registration drop, an unrouted #[tool] fn, or a same-count rename
# fail CI instead of degrading to "tool not found" at dispatch.
#
# The pin's two tests (count + TOOL_NAMES-vs-router) are the mechanical
# enforcement of the registration pattern; this gate enforces the pin's
# PRESENCE so no server can ship without it (the 2026-10-05 audit found
# portfolio/experimentation/spreadsheet without it and 9 servers with
# count-only pins that could not catch renames).
set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
LIST="$ROOT/kask/scripts/build/mcp-servers.txt"

if [ ! -f "$LIST" ]; then
    echo "error: server list not found: $LIST" >&2
    exit 2
fi

fail=0
count=0
while IFS= read -r pkg; do
    case "$pkg" in
        ''|'#'*) continue ;;
    esac
    count=$((count + 1))
    dir="$ROOT/kask/mcp-servers/$pkg"
    if [ ! -d "$dir" ]; then
        echo "FAIL: $pkg — server directory not found: $dir"
        fail=$((fail + 1))
        continue
    fi
    if grep -rqn "tool_surface_pin!" "$dir/src/" 2>/dev/null; then
        echo "PASS: $pkg"
    else
        echo "FAIL: $pkg — no tool_surface_pin! invocation in src/ (a registration change here would not be pinned)"
        fail=$((fail + 1))
    fi
done < "$LIST"

echo "----"
echo "servers checked: $count  failed: $fail"
[ "$fail" -eq 0 ]
