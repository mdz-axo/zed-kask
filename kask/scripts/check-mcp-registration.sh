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
# servers without it and 9 servers with count-only pins that could not
# catch renames).
#
# Detection is syntax-aware, not textual. A plain `grep tool_surface_pin!`
# is satisfied by a COMMENTED-OUT pin, by the macro name inside a string
# literal, and by the prose mentions three servers already carry above
# their real pin — so the cheapest way to disable the pin (comment it out)
# left the gate green. The awk pass below strips line comments, block
# comments, and string literals before matching, then requires an actual
# invocation: an optional `path::` qualifier, the macro name, `!`, and an
# opening delimiter. Proven against all real invocation shapes in-tree
# (bare, `hkask_mcp_server::`-qualified, multi-line, brace-delimited) and
# against the four bypass shapes, by check-mcp-registration-selftest.sh.
set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
LIST="${LIST:-$ROOT/kask/scripts/build/mcp-servers.txt}"
SERVERS_DIR="${SERVERS_DIR:-$ROOT/kask/mcp-servers}"

if [ ! -f "$LIST" ]; then
    echo "error: server list not found: $LIST" >&2
    exit 2
fi

# Emit `file:line` for every real tool_surface_pin! invocation in the files
# given as arguments. Comments and string literals are removed first, so a
# disabled pin cannot satisfy the gate.
has_real_pin() {
    awk '
      {
        line = $0
        out = ""
        while (length(line) > 0) {
          if (inblock) {
            i = index(line, "*/")
            if (i == 0) { line = ""; break }
            line = substr(line, i + 2); inblock = 0; continue
          }
          lc = index(line, "//"); bo = index(line, "/*"); sq = index(line, "\"")
          best = 0; kind = ""
          if (lc > 0)                             { best = lc; kind = "lc" }
          if (bo > 0 && (best == 0 || bo < best)) { best = bo; kind = "bo" }
          if (sq > 0 && (best == 0 || sq < best)) { best = sq; kind = "sq" }
          if (best == 0) { out = out line; line = ""; break }
          out = out substr(line, 1, best - 1)
          if (kind == "lc") { line = ""; break }
          if (kind == "bo") { line = substr(line, best + 2); inblock = 1; continue }
          rest = substr(line, best + 1); j = 1; closed = 0
          while (j <= length(rest)) {
            c = substr(rest, j, 1)
            if (c == "\\") { j += 2; continue }
            if (c == "\"") { closed = 1; break }
            j++
          }
          if (!closed) { line = ""; break }
          line = substr(rest, j + 1)
        }
        if (out ~ /(^|[^A-Za-z0-9_])([A-Za-z_][A-Za-z0-9_]*::)*tool_surface_pin![ \t]*[({[]/) {
          print FILENAME ":" FNR
        }
      }
    ' "$@"
}

fail=0
count=0
while IFS= read -r pkg; do
    case "$pkg" in
        ''|'#'*) continue ;;
    esac
    count=$((count + 1))
    dir="$SERVERS_DIR/$pkg"
    if [ ! -d "$dir" ]; then
        echo "FAIL: $pkg — server directory not found: $dir"
        fail=$((fail + 1))
        continue
    fi
    sources=$(find "$dir/src" -name '*.rs' 2>/dev/null || true)
    if [ -z "$sources" ]; then
        echo "FAIL: $pkg — no .rs sources under $dir/src"
        fail=$((fail + 1))
        continue
    fi
    # shellcheck disable=SC2086 # intentional word-split over the file list
    site=$(has_real_pin $sources 2>/dev/null | head -1)
    if [ -n "$site" ]; then
        echo "PASS: $pkg ($site)"
    else
        if grep -rqn "tool_surface_pin!" "$dir/src/" 2>/dev/null; then
            echo "FAIL: $pkg — tool_surface_pin! appears only in a comment or string literal, not as an invocation (a disabled pin is an unpinned registration)"
        else
            echo "FAIL: $pkg — no tool_surface_pin! invocation in src/ (a registration change here would not be pinned)"
        fi
        fail=$((fail + 1))
    fi
done < "$LIST"

echo "----"
echo "servers checked: $count  failed: $fail"
[ "$fail" -eq 0 ]
