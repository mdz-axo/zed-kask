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
#     blocks. The returned NAME is always re-findable by --find:
#     a method inside an impl returns "Type::method" (disambiguated),
#     an impl block returns "impl Type", everything else returns its
#     plain name. A doc-comment line between an impl's methods resolves
#     to "impl Type". Known limit: a top-level doc comment or blank
#     line has no enclosing node (exit 1).
#   resolve-citation-anchor.sh --find FILE NAME
#     Prints the CURRENT start line of the definition named NAME;
#     exit 0. Exit 1 when not found; exit 2 when ambiguous. NAME forms:
#     plain ("beta", "std::collections::HashMap" for use paths),
#     "Type::method" (impl-qualified; generic impls normalized, so
#     `impl<T> Foo<T>` answers to `Foo::method`; qualification is by
#     the IMPLEMENTED type — `impl Tr for Foo` answers to `Foo::...`),
#     "impl Type" (an impl block). A single-"::" NAME that matches no
#     impl falls back to the plain-definitions search, so use paths
#     like "std::fmt" still resolve.
#
# Exit 3 is a usage/tooling error (bad args, missing tool).
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
AST_GREP="$SCRIPT_DIR/ast-grep.sh"
DEFS_RULE="$SCRIPT_DIR/../ast-grep/rules/rust-definitions.yml"
IMPLS_RULE="$SCRIPT_DIR/../ast-grep/rules/rust-impls.yml"

usage() {
  echo "usage: $0 FILE LINE | $0 --find FILE NAME | $0 --find FILE 'Type::method' | $0 --find FILE 'impl Type'" >&2
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

# "impl TYPE"<TAB>START<TAB>END per impl block — the "impl " prefix
# (impossible in a Rust identifier) keeps impl rows distinguishable
# from item rows in the combined stream.
impls() {
  "$AST_GREP" scan --rule "$IMPLS_RULE" --json=stream "$1" 2>/dev/null \
    | jq -r 'select(.ruleId == "rust-impls")
             | ("impl " + .metaVariables.single.TYPE.text) + "\t\(.range.start.line + 1)\t\(.range.end.line + 1)"' \
    || true
}

# The plain-definitions arm of --find: exact name match over defs().
find_plain() {
  defs "$1" | awk -F'\t' -v N="$2" '$1 == N { print $2 }'
}

# The impl-qualified arm: methods named M inside impl blocks of type T
# (generics normalized off both sides).
find_qualified() {
  local file=$1 qtype=$2 qmethod=$3
  local impl_spans
  impl_spans=$(impls "$file" | awk -F'\t' -v QT="$qtype" '{ t = $1; sub(/^impl /, "", t); sub(/<.*/, "", t); if (t == QT) print $2 "\t" $3 }')
  [ -n "$impl_spans" ] || return 1
  defs "$file" | awk -F'\t' -v M="$qmethod" '$1 == M { print $2 }' \
    | while IFS= read -r ds; do
        while IFS=$'\t' read -r is ie; do
          if [ "$ds" -ge "$is" ] && [ "$ds" -le "$ie" ]; then
            printf '%s\n' "$ds"
          fi
        done <<< "$impl_spans"
      done | sort -u
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
    if [[ "$name" == "impl "* ]]; then
      qtype=$(printf '%s' "${name#impl }" | sed 's/<.*//')
      hits=$(impls "$file" | awk -F'\t' -v QT="$qtype" '{ t = $1; sub(/^impl /, "", t); sub(/<.*/, "", t); if (t == QT) print $2 }')
    elif [[ "$name" == *"::"* ]]; then
      qtype=$(printf '%s' "${name%%::*}" | sed 's/<.*//')
      qmethod=${name##*::}
      hits=$(find_qualified "$file" "$qtype" "$qmethod") || hits=""
      # A single-"::" name that matches no impl may be a use path
      # (e.g. "std::fmt") — fall back to the plain-definitions search.
      [ -z "$hits" ] && hits=$(find_plain "$file" "$name")
    else
      hits=$(find_plain "$file" "$name")
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
    fname=$(printf '%s' "$found" | cut -f1)
    fstart=$(printf '%s' "$found" | cut -f2)
    if [[ "$fname" == "impl "* ]]; then
      printf '%s\t%s\n' "$fname" "$fstart"
    else
      # A definition inside an impl returns "Type::name" so --find can
      # re-find it without ambiguity.
      qual=""
      while IFS=$'\t' read -r iname istart iend; do
        [[ "$iname" == "impl "* ]] || continue
        it=${iname#impl }; it=${it%%<*}
        if [ "$fstart" -ge "$istart" ] && [ "$fstart" -le "$iend" ]; then
          qual=$it
          break
        fi
      done <<< "$(impls "$file")"
      if [ -n "$qual" ]; then
        printf '%s::%s\t%s\n' "$qual" "$fname" "$fstart"
      else
        printf '%s\t%s\n' "$fname" "$fstart"
      fi
    fi
    ;;
esac