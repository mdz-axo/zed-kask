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
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIST="${LIST:-$ROOT/kask/scripts/build/mcp-servers.txt}"
README="${README:-$ROOT/kask/docs/reference/mcp-servers/README.md}"
SERVERS_DIR="${SERVERS_DIR:-$ROOT/kask/mcp-servers}"
AST_GREP="${AST_GREP:-$HERE/ast-grep.sh}"
PIN_RULE="${PIN_RULE:-$HERE/../ast-grep/rules/tool-surface-pin.yml}"

[ -f "$LIST" ] || { echo "error: server list not found: $LIST" >&2; exit 2; }
[ -f "$README" ] || { echo "error: README not found: $README" >&2; exit 2; }
[ -d "$SERVERS_DIR" ] || { echo "error: servers dir not found: $SERVERS_DIR" >&2; exit 2; }
[ -x "$AST_GREP" ] || { echo "error: structural parser not executable: $AST_GREP" >&2; exit 2; }
[ -f "$PIN_RULE" ] || { echo "error: structural rule not found: $PIN_RULE" >&2; exit 2; }
command -v jq >/dev/null 2>&1 || { echo "error: jq is required" >&2; exit 2; }

PIN_RAW=$(mktemp)
PIN_ERR=$(mktemp)
PIN_TSV=$(mktemp)
trap 'rm -f "$PIN_RAW" "$PIN_ERR" "$PIN_TSV"' EXIT

"$AST_GREP" scan --rule "$PIN_RULE" --json=stream "$SERVERS_DIR" >"$PIN_RAW" 2>"$PIN_ERR"
sg_rc=$?
if [ "$sg_rc" -ne 0 ]; then
    echo "error: ast-grep failed scanning $SERVERS_DIR (exit $sg_rc):" >&2
    cat "$PIN_ERR" >&2
    exit 2
fi
if ! jq -r '[.file, (.text | gsub("[\\r\\n\\t]+"; " "))] | @tsv' "$PIN_RAW" >"$PIN_TSV"; then
    echo "error: jq could not parse ast-grep output" >&2
    exit 2
fi

parse_pin_macro() {
    awk '
    {
      s = $0
      if (!match(s, /tool_surface_pin[[:space:]]*![[:space:]]*[\(\[\{]/)) {
        print "ERR\tunparseable"
        exit
      }
      body = substr(s, RSTART + RLENGTH)
      sub(/[;[:space:]]+$/, "", body)
      sub(/[\)\]\}]$/, "", body)
      n = 0; cur = ""; depth = 0; in_str = 0; esc = 0
      for (i = 1; i <= length(body); i++) {
        c = substr(body, i, 1)
        if (in_str) {
          cur = cur c
          if (esc) { esc = 0 }
          else if (c == "\\") { esc = 1 }
          else if (c == "\"") { in_str = 0 }
          continue
        }
        if (c == "\"") { in_str = 1; cur = cur c; continue }
        if (c == "(" || c == "[" || c == "{") { depth++; cur = cur c; continue }
        if (c == ")" || c == "]" || c == "}") { if (depth > 0) depth--; cur = cur c; continue }
        if (c == "," && depth == 0) {
          gsub(/^[[:space:]]+|[[:space:]]+$/, "", cur)
          n++; args[n] = cur; cur = ""
          continue
        }
        cur = cur c
      }
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", cur)
      if (cur != "") { n++; args[n] = cur }
      if (n == 2) {
        printf "OK2\t%s\n", args[2]
      } else if (n == 4) {
        printf "OK4\t%s\t%s\n", args[3], args[4]
      } else {
        printf "ERR\tarity_%d\n", n
      }
    }'
}

fail=0
checked_servers=0
declare -a pins=()

while IFS= read -r pkg; do
    case "$pkg" in ''|'#'*) continue ;; esac
    checked_servers=$((checked_servers + 1))
    id=${pkg#hkask-mcp-}
    src_dir="$SERVERS_DIR/$pkg/src"
    if [ ! -d "$src_dir" ]; then
        echo "FAIL: $id — src dir not found: $src_dir"
        fail=$((fail + 1))
        continue
    fi
    pin_row=$(awk -F'\t' -v prefix="$src_dir/" 'index($1, prefix) == 1 { print; exit }' "$PIN_TSV")
    if [ -z "$pin_row" ]; then
        echo "FAIL: $id — no tool_surface_pin! invocation under $src_dir"
        fail=$((fail + 1))
        continue
    fi
    pin_text=${pin_row#*$'\t'}
    parsed=$(printf '%s\n' "$pin_text" | parse_pin_macro)
    pin_kind=$(printf '%s' "$parsed" | cut -f1)
    arg_n=$(printf '%s' "$parsed" | cut -f2)
    pin_sym=$(printf '%s' "$parsed" | cut -f3)

    if [ "$pin_kind" != "OK2" ] && [ "$pin_kind" != "OK4" ]; then
        echo "FAIL: $id — unsupported tool_surface_pin! shape ($arg_n): $pin_text"
        fail=$((fail + 1))
        continue
    fi
    case "$arg_n" in
        ''|*[!0-9]*)
            echo "FAIL: $id — tool_surface_pin! count argument is not a bare integer (got '$arg_n')"
            fail=$((fail + 1))
            continue
            ;;
    esac
    if [ "$pin_kind" = "OK4" ]; then
        name_n=$(printf '%s' "$pin_sym" | grep -oE '[0-9]+' | head -1)
        if [ -z "$name_n" ]; then
            echo "FAIL: $id — 4-arg tool_surface_pin! test name '$pin_sym' embeds no tool count"
            fail=$((fail + 1))
            continue
        fi
        if [ "$name_n" != "$arg_n" ]; then
            echo "FAIL: $id — the pin test name says $name_n but the count argument says $arg_n"
            fail=$((fail + 1))
            continue
        fi
    fi
    pins+=("$arg_n")

    # Surface 2: the Live pin sites entry — N matches the pin, the cited
    # file exists, and (for 4-arg pins, or whenever cited) the pin-test symbol
    # occurs in it with the same embedded count.
    entry=$(grep -oE "(^|[^a-z0-9_-])$id $arg_n \([^)]+\)" "$README" | sed -E 's/^[^a-z0-9]+//' | head -1)
    if [ -z "$entry" ]; then
        readme_n=$(grep -oE "(^|[^a-z0-9_-])$id [0-9]+ \(" "$README" | sed -E 's/^[^a-z0-9]+//' | grep -oE '[0-9]+' | head -1)
        echo "FAIL: $id — Live pin sites entry says ${readme_n:-<missing>} but the pin says $arg_n"
        fail=$((fail + 1))
    else
        cited_file=$(printf '%s' "$entry" | grep -oE '[a-z_0-9]+\.rs' | head -1)
        cited_sym=$(printf '%s' "$entry" | grep -oE 'tool_surface_is_exactly_[0-9]+_registered_tools?' | head -1)
        resolved_cited=""
        if [ -n "$cited_file" ]; then
            if [ -f "$src_dir/$cited_file" ]; then
                resolved_cited="$src_dir/$cited_file"
            else
                resolved_cited=$(find "$src_dir" -type f -name "$cited_file" 2>/dev/null | sort | head -1)
            fi
        fi
        if [ -z "$cited_file" ] || { [ "$pin_kind" = "OK4" ] && [ -z "$cited_sym" ]; }; then
            echo "FAIL: $id — Live pin sites entry does not name the pin test (expected file.rs and tool_surface_is_exactly_N_registered_tools)"
            fail=$((fail + 1))
        elif [ -z "$resolved_cited" ] || [ ! -f "$resolved_cited" ]; then
            echo "FAIL: $id — pin-site citation names a missing file: $cited_file"
            fail=$((fail + 1))
        elif [ -n "$cited_sym" ] && ! grep -qF -- "$cited_sym" "$resolved_cited"; then
            echo "FAIL: $id — pin-site symbol $cited_sym not found in $cited_file (renamed?)"
            fail=$((fail + 1))
        elif [ -n "$cited_sym" ]; then
            sym_n=$(printf '%s' "$cited_sym" | sed -E 's/tool_surface_is_exactly_([0-9]+)_registered_tools?/\1/')
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

if [ "$checked_servers" -eq 0 ] || [ "${#pins[@]}" -eq 0 ]; then
    echo "FAIL: zero server pins checked from $LIST — refusing to pass vacuously"
    fail=$((fail + 1))
fi

echo "----"
echo "README count surfaces checked: ${#pins[@]} servers  failed: $fail"
[ "$fail" -eq 0 ]