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
# Detection is structural. The repository-pinned ast-grep parser matches a
# Rust `macro_invocation` whose macro path ends in `tool_surface_pin`, so
# comments and strings cannot satisfy the gate, legal whitespace before `!`
# remains valid, and all three Rust macro delimiters are accepted. The rule
# excludes pins nested in a module unconditionally compiled out with
# `#[cfg(any())]`.
#
# Structural presence is only half the contract: the kask-tests CI job runs
# the macro-generated `tool_names_match_live_router` test once per listed
# server through cargo-test-nonzero.sh. That per-server liveness leg catches
# direct cfg-disablement and any other condition that prevents the pin from
# generating a test.
set -euo pipefail

ROOT=$(git rev-parse --show-toplevel)
LIST="${LIST:-$ROOT/kask/scripts/build/mcp-servers.txt}"
SERVERS_DIR="${SERVERS_DIR:-$ROOT/kask/mcp-servers}"

AST_GREP="$ROOT/kask/scripts/ast-grep.sh"
RULE_FILE="$ROOT/kask/ast-grep/rules/tool-surface-pin.yml"

if [ ! -f "$LIST" ]; then
    echo "error: server list not found: $LIST" >&2
    exit 2
fi
if [ ! -x "$AST_GREP" ]; then
    echo "error: structural parser not executable: $AST_GREP" >&2
    exit 2
fi
if [ ! -f "$RULE_FILE" ]; then
    echo "error: structural rule not found: $RULE_FILE" >&2
    exit 2
fi
command -v jq >/dev/null 2>&1 || {
    echo "error: jq is required to read ast-grep's JSON output" >&2
    exit 2
}

# Emit `file:line` for real tool_surface_pin! invocations. ast-grep uses exit
# 0 for hint-level matches and no matches alike; parser/configuration failures
# remain nonzero and are surfaced rather than degraded to "no pin".
find_real_pins() {
    local output errors status
    output=$(mktemp)
    errors=$(mktemp)
    set +e
    "$AST_GREP" scan --rule "$RULE_FILE" --json=stream "$@" >"$output" 2>"$errors"
    status=$?
    set -e
    if [ "$status" -ne 0 ]; then
        echo "error: ast-grep could not inspect the server sources:" >&2
        cat "$errors" >&2
        rm -f "$output" "$errors"
        return 2
    fi
    if ! jq -r '"\(.file):\(.range.start.line + 1)"' "$output"; then
        echo "error: could not parse ast-grep output" >&2
        rm -f "$output" "$errors"
        return 2
    fi
    rm -f "$output" "$errors"
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
    mapfile -t sources < <(find "$dir/src" -name '*.rs' -type f 2>/dev/null | sort)
    if [ "${#sources[@]}" -eq 0 ]; then
        echo "FAIL: $pkg — no .rs sources under $dir/src"
        fail=$((fail + 1))
        continue
    fi
    set +e
    pin_sites=$(find_real_pins "${sources[@]}")
    pin_status=$?
    set -e
    if [ "$pin_status" -ne 0 ]; then
        exit "$pin_status"
    fi
    site=$(printf '%s\n' "$pin_sites" | sed -n '1p')
    if [ -n "$site" ]; then
        echo "PASS: $pkg ($site)"
    else
        echo "FAIL: $pkg — no live tool_surface_pin! invocation in src/ (a registration change here would not be pinned)"
        fail=$((fail + 1))
    fi
done < "$LIST"

echo "----"
echo "servers checked: $count  failed: $fail"
[ "$fail" -eq 0 ]
