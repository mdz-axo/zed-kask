#!/usr/bin/env bash
#
# ADVISORY AGENT TOOL — unwired from push/CI enforcement by operator
# mandate 2026-10-31 (the rules-that-don't-improve-code-quality cut).
# Run it voluntarily; nothing blocks a push or fails CI on it. Re-wiring
# it into kask/githooks/pre-push or .github/workflows/kask-invariants.yml
# requires an explicit operator decision.
# check-mcp-readme-counts.sh — README count-consistency gate (added
# 2026-10-30, the media audit's third recurrence of the drift class: the
# S2/S3 commits moved the media pin to 90 while the README's prose counts
# stayed at 88; the layers_transcript commit and the pre-t5 streams
# missed it before that).
#
# The registration pin (tool_surface_pin!, enforced by
# check-mcp-registration.sh) is the authoritative tool count. This gate
# verifies the README's three PROSE count surfaces agree with it, so a
# registration change that forgets the README fails the gate instead of
# shipping silently:
#   1. the catalog table row's trailing `| N |` for the server,
#   2. the "Live pin sites" entry `<id> N (`file.rs` → `pin-test symbol`)`
#      — N matches the pin, the cited file exists, and the cited
#      tool_surface_is_exactly_N_registered_tools symbol occurs in that
#      file with the same embedded count (symbol form since 2026-10-30:
#      the former `file:line` form carried no invariant — any edit above
#      the cited line silently rotted the citation, the kata-kanban
#      1632→1630 drift this gate caught on its first push),
#   3. the "Arithmetic" line — its terms are exactly the live pin counts
#      and its stated total is their sum.
#
# The authoritative count is the pin macro's integer argument (the line
# above the test-name argument); the test name's embedded number must
# agree with it (name-arg drift fails loudly, not silently).
#
# Self-test: kask/scripts/check-mcp-readme-counts-selftest.sh (fixture
# README/server pair asserting every drift class blocks and consistency
# passes). Runs in CI via the check-*-selftest.sh glob.
set -uo pipefail

ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
LIST="${LIST:-$ROOT/kask/scripts/build/mcp-servers.txt}"
README="${README:-$ROOT/kask/docs/reference/mcp-servers/README.md}"
SERVERS_DIR="${SERVERS_DIR:-$ROOT/kask/mcp-servers}"

[ -f "$LIST" ] || { echo "error: server list not found: $LIST" >&2; exit 2; }
[ -f "$README" ] || { echo "error: README not found: $README" >&2; exit 2; }

fail=0
declare -a pins=()

while IFS= read -r pkg; do
    case "$pkg" in ''|'#'*) continue ;; esac
    id=${pkg#hkask-mcp-}
    lib="$SERVERS_DIR/$pkg/src/$(printf '%s' "$pkg" | tr '-' '_').rs"
    if [ ! -f "$lib" ]; then
        echo "FAIL: $id — lib root not found: $lib"
        fail=$((fail + 1))
        continue
    fi
    # The pin's test-name argument: trailing comma at end of line, so the
    # doc-comment mentions of the pin name do not match.
    name_line=$(grep -nE 'tool_surface_is_exactly_[0-9]+_registered_tools,$' "$lib" | head -1 | cut -d: -f1)
    if [ -z "$name_line" ]; then
        echo "FAIL: $id — no tool_surface_pin! test-name argument in $lib"
        fail=$((fail + 1))
        continue
    fi
    name_n=$(sed -n "${name_line}p" "$lib" | grep -oE '[0-9]+' | head -1)
    arg_n=$(sed -n "$((name_line - 1))p" "$lib" | tr -d ' \t,')
    case "$arg_n" in
        ''|*[!0-9]*)
            echo "FAIL: $id — the line above the pin test name is not a bare integer count argument (got '$arg_n')"
            fail=$((fail + 1))
            continue
            ;;
    esac
    if [ "$name_n" != "$arg_n" ]; then
        echo "FAIL: $id — the pin test name says $name_n but the count argument says $arg_n"
        fail=$((fail + 1))
        continue
    fi
    pins+=("$arg_n")

    # Surface 2: the Live pin sites entry — N matches the pin, the cited
    # file exists, and the cited pin-test symbol occurs in it with the
    # same embedded count. Symbol form (2026-10-30): the former file:line
    # form carried no invariant — any edit above the cited line silently
    # rotted the citation (the kata-kanban 1632→1630 drift this gate
    # caught on its first push); the symbol names the pin test and
    # cannot drift.
    entry=$(grep -oE "$id $arg_n \([^)]+\)" "$README" | head -1)
    if [ -z "$entry" ]; then
        readme_n=$(grep -oE "$id [0-9]+ \(" "$README" | grep -oE '[0-9]+' | head -1)
        echo "FAIL: $id — Live pin sites entry says ${readme_n:-<missing>} but the pin says $arg_n"
        fail=$((fail + 1))
    else
        cited_file=$(printf '%s' "$entry" | grep -oE '[a-z_0-9]+\.rs' | head -1)
        cited_sym=$(printf '%s' "$entry" | grep -oE 'tool_surface_is_exactly_[0-9]+_registered_tools' | head -1)
        if [ -z "$cited_file" ] || [ -z "$cited_sym" ]; then
            echo "FAIL: $id — Live pin sites entry does not name the pin test (expected file.rs and tool_surface_is_exactly_N_registered_tools)"
            fail=$((fail + 1))
        elif [ ! -f "$SERVERS_DIR/$pkg/src/$cited_file" ]; then
            echo "FAIL: $id — pin-site citation names a missing file: $cited_file"
            fail=$((fail + 1))
        elif ! grep -qF -- "$cited_sym" "$SERVERS_DIR/$pkg/src/$cited_file"; then
            echo "FAIL: $id — pin-site symbol $cited_sym not found in $cited_file (renamed?)"
            fail=$((fail + 1))
        else
            sym_n=$(printf '%s' "$cited_sym" | sed -E 's/tool_surface_is_exactly_([0-9]+)_registered_tools/\1/')
            if [ "$sym_n" != "$arg_n" ]; then
                echo "FAIL: $id — pin-test symbol says $sym_n but the pin says $arg_n"
                fail=$((fail + 1))
            fi
        fi
    fi

    # Surface 1: the catalog table row.
    row=$(grep -E "kask/mcp-servers/$pkg\`" "$README" | grep -E '\| [0-9]+ \|$' | head -1)
    if [ -z "$row" ]; then
        echo "FAIL: $id — no catalog table row naming $pkg"
        fail=$((fail + 1))
    else
        row_n=$(printf '%s' "$row" | grep -oE '\| [0-9]+ \|$' | grep -oE '[0-9]+' | head -1)
        if [ "$row_n" != "$arg_n" ]; then
            echo "FAIL: $id — catalog table row says ${row_n:-<none>} but the pin says $arg_n"
            fail=$((fail + 1))
        fi
    fi
done < "$LIST"

# Surface 3: the arithmetic line — terms are exactly the live pins and
# the stated total is their sum.
arith=$(grep -oE 'Arithmetic: `[0-9+ ]+=[ 0-9]+`\.' "$README" | head -1)
if [ -z "$arith" ]; then
    echo "FAIL: no Arithmetic line in $README"
    fail=$((fail + 1))
else
    terms=$(printf '%s' "$arith" | sed -E 's/^Arithmetic: `//; s/`\.$//' | tr '+' ' ' | cut -d= -f1)
    total=$(printf '%s' "$arith" | grep -oE '=[ ]*[0-9]+' | grep -oE '[0-9]+')
    sum=0
    for term in $terms; do sum=$((sum + term)); done
    pin_sum=0
    for pin in "${pins[@]}"; do pin_sum=$((pin_sum + pin)); done
    case "$total" in
        ''|*[!0-9]*)
            echo "FAIL: arithmetic — stated total is not a bare integer (got '$total')"
            fail=$((fail + 1))
            ;;
        *)
            if [ "$sum" -ne "$total" ] || [ "$sum" -ne "$pin_sum" ]; then
                echo "FAIL: arithmetic — terms sum to $sum, stated total $total, live pin sum $pin_sum"
                fail=$((fail + 1))
            fi
            ;;
    esac
    t_sorted=$(printf '%s\n' $terms | sort -n | tr '\n' ' ')
    p_sorted=$(printf '%s\n' "${pins[@]}" 2>/dev/null | sort -n | grep -vE '^$' | tr '\n' ' ')
    if [ "$t_sorted" != "$p_sorted" ]; then
        echo "FAIL: arithmetic — terms [$t_sorted] are not the live pin counts [$p_sorted]"
        fail=$((fail + 1))
    fi
fi

echo "----"
echo "README count surfaces checked: ${#pins[@]} servers  failed: $fail"
[ "$fail" -eq 0 ]