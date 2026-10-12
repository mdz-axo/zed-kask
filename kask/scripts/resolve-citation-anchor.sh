#!/usr/bin/env bash
# resolve-citation-anchor.sh — symbol-anchored citation resolution (UC-3
# of the tree-sitter adoption proposal,
# kask/docs/research/tree-sitter-adoption-proposal.md).
#
# A `file:line` citation rots when lines shift above it (.rules: "a
# line-shifting edit ... silently invalidates every doc citing those
# lines"). The resolver re-anchors mechanically: the innermost Rust
# definition enclosing the cited line, resolved structurally via
# tree-sitter (kask/ast-grep/rules/rust-definitions.yml +
# rust-impls.yml), so a drifted citation can be repaired to the
# definition's CURRENT line without hand-searching.
#
# Usage:
#   resolve-citation-anchor.sh FILE LINE
#     Prints "NAME<TAB>START_LINE" (1-based) of the innermost definition
#     enclosing LINE; exit 0. Exit 1 when no definition encloses the
#     line. Candidates: functions (incl. trait signatures), structs,
#     enums, traits, mods, consts, statics, type aliases, macro_rules,
#     extern crate, use paths (keyed by FULL path text), and impl
#     blocks (keyed by implemented type) — a doc-comment line between
#     an impl's methods resolves to the impl.
#   resolve-citation-anchor.sh --find FILE NAME
#     Prints the CURRENT start line of the definition named NAME;
#     exit 0. Exit 1 when not found; exit 2 when ambiguous.
#   resolve-citation-anchor.sh --find FILE 'Type::method'
#     Impl-qualified disambiguation (2026-10-11): resolves a method
#     defined in several impl blocks by the implemented type. Generic
#     impls are normalized (`impl<T> Foo<T>` answers to `Foo::method`).
#     Qualification is by the IMPLEMENTED type, not the trait
#     (`impl Tr for Foo` answers to `Foo::...`).
#
# Exit 3 is a usage/tooling error (bad args, missing tool).
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
AST_GREP="$SCRIPT_DIR/ast-grep.sh"
DEFS_RULE="$SCRIPT_DIR/../ast-grep/rules/rust-definitions.yml"
IMPLS_RULE="$SCRIPT_DIR/../ast-grep/rules/rust-impls.yml"

usage() {
  echo "usage: $0 FILE LINE | $0 --find FILE NAME | $0 --find FILE 'Type::method'" >&2
  exit 3
}

command -v jq >/dev/null 2>&1 || {
  echo "FAIL: jq not found — the resolver parses ast-grep's stream JSON with jq." >&2
  exit 3
}
[ -x "$AST_GREP" ] || {
  echo "FAIL: $AST_GREP missing or not executable." >&2
  exit 3
}
[ -f "$DEFS_RULE" ] || {
  echo "FAIL: rule file $DEFS_RULE missing." >&2
  exit 3
}
[ -f "$IMPLS_RULE" ] || {
  echo "FAIL: rule file $IMPLS_RULE missing." >&2
  exit 3
}

# NAME<TAB>START<TAB>END per definition (1-based lines). Stream JSON on
# stdout, diagnostics on stderr, exit 1 possible (info severity — the
# redirect and || true keep it quiet).
defs() {
  "$AST_GREP" scan --rule "$DEFS_RULE" --json=stream "$1" 2>/dev/null \
    | jq -r 'select(.ruleId == "rust-definitions")
             | "\(.metaVariables.single.NAME.text)\t\(.range.start.line + 1)\t\(.range.end.line + 1)"' \
    || true
}

# TYPE<TAB>START<TAB>END per impl block.
impls() {
  "$AST_GREP" scan --rule "$IMPLS_RULE" --json=stream "$1" 2>/dev/null \
    | jq -r 'select(.ruleId == "rust-impls")
             | "\(.metaVariables.single.TYPE.text)\t\(.range.start.line + 1)\t\(.range.end.line + 1)"' \
    || true
}

case "${1:-}" in
  --find)
    [ $# -eq 3 ] || usage
    file=$2
    name=$3
    [ -f "$file" ] || {
      echo "FAIL: file not found: $file" >&2
      exit 3
    }
    if [[ "$name" == *"::"* && "$name" != *"::"*"::"* ]]; then
      # Impl-qualified: Type::method. Normalize generics off both sides
      # (`impl<T> Foo<T>` answers to `Foo::method`).
      qtype=$(printf '%s' "${name%%::*}" | sed 's/<.*//')
      qmethod=${name##*::}
      impl_spans=$(impls "$file" | awk -F'\t' -v QT="$qtype" '{ t = $1; sub(/<.*/, "", t); if (t == QT) print $2 "\t" $3 }')
      if [ -z "$impl_spans" ]; then
        echo "no impl block for type '$qtype' in $file" >&2
        exit 1
      fi
      hits=$(defs "$file" | awk -F'\t' -v M="$qmethod" '$1 == M { print $2 }' \
        | while IFS= read -r ds; do
            while IFS=$'\t' read -r is ie; do
              if [ "$ds" -ge "$is" ] && [ "$ds" -le "$ie" ]; then
                printf '%s\n' "$ds"
              fi
            done <<< "$impl_spans"
          done | sort -u)
    else
      hits=$(defs "$file" | awk -F'\t' -v N="$name" '$1 == N { print $2 }')
    fi
    count=$(printf '%s\n' "$hits" | grep -c . || true)
    if [ "$count" -eq 0 ]; then
      echo "no definition named '$name' in $file" >&2
      exit 1
    elif [ "$count" -gt 1 ]; then
      echo "ambiguous: $count definitions named '$name' in $file:" >&2
      printf '%s\n' "$hits" >&2
      exit 2
    fi
    printf '%s\n' "$hits"
    ;;
  *)
    [ $# -eq 2 ] || usage
    file=$1
    line=$2
    case "$line" in
      ''|*[!0-9]*) usage ;;
    esac
    [ -f "$file" ] || {
      echo "FAIL: file not found: $file" >&2
      exit 3
    }
    # Innermost enclosing definition = the candidate with the greatest
    # start line among those whose span contains the cited line (spans
    # nest in a tree, so the greatest start is the innermost). Impls are
    # candidates too: a doc-comment line between an impl's methods
    # resolves to the impl.
    found=$({ defs "$file"; impls "$file"; } \
      | awk -F'\t' -v L="$line" '$2 <= L && L <= $3 && $2 > best { best = $2; n = $1; s = $2 } END { if (n != "") print n "\t" s }')
    if [ -z "$found" ]; then
      echo "no enclosing definition at $file:$line" >&2
      exit 1
    fi
    printf '%s\n' "$found"
    ;;
esac