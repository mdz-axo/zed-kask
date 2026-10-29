#!/usr/bin/env bash
# Content-aware docs-citation gate (the A-2 finding from the 2026-10-18
# architecture review; operator ruling 2026-10-18, option (a) scoped small).
#
# THE GAP THIS CLOSES: check-docs-citations.sh validates that a cited
# `path.rs:line` resolves to a LIVE file and that the line EXISTS. It cannot
# see whether the line still CONTAINS the thing the doc describes. A
# line-shifting edit above a citation silently re-points it at unrelated code
# while the gate stays green — measured live 2026-10-18: commit caf2e15bcb
# inserted ~200 lines into mcp_servers.rs and rotted 16 citations across 8
# docs, with the gate reporting "0 rotted".
#
# THE MECHANISM: a citation MAY carry an expected-symbol annotation —
# `path.rs:LINE@symbol` — naming the definition the line should sit inside.
# The gate resolves the enclosing definition at LINE (structurally, via
# kask/scripts/resolve-citation-anchor.sh, the tree-sitter resolver) and fails
# when the name does not match. A rotted pointer fails loudly; a
# wrong-from-the-start pointer fails immediately.
#
# FORWARD-LOOKING, like the D/P labelling convention: an UNANNOTATED citation
# into a scoped file is a WARNING (grandfathered — the existing corpus is not
# retrofitted in one pass), an annotated citation whose symbol does not match
# is a FAILURE. The scoped set is the high-churn files where rot is measured;
# annotate a citation when you touch it.
#
# NAME MATCHING: the resolver returns "Type::method" for a def inside an impl,
# "impl Type" for an impl block, else the plain name. A citation may annotate
# with either the full qualified name or the bare method name (the bare name
# matches the "::" tail), so the common case stays short.
#
# Run locally: bash kask/scripts/check-docs-citation-anchors.sh
# Exit codes: 0 = no mismatches (warnings allowed), 1 = an annotated citation
#             does not match its symbol, 2 = usage/tooling.

set -uo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "FAIL: not inside a git repository (run from within the repo)." >&2
  exit 2
}
cd "$REPO_ROOT" || exit 2
DOCS_ROOT="${DOCS_ROOT:-kask/docs}"
RESOLVER="${RESOLVER:-kask/scripts/resolve-citation-anchor.sh}"

[ -f "$RESOLVER" ] || {
  echo "FAIL: resolver $RESOLVER not found (the gate resolves enclosing defs structurally)." >&2
  exit 2
}
[ -d "$DOCS_ROOT" ] || {
  echo "FAIL: docs root $DOCS_ROOT not found." >&2
  exit 2
}

# The scoped set: high-churn files where citation rot is measured (git
# churn over 60 days, 2026-10-18). Citations into these files are checked
# for the @symbol annotation; the rest of the corpus is out of scope (the
# gate is the mechanical floor, not the whole-corpus retrofit).
SCOPED_BASENAMES=(
  main.rs
  mcp_servers.rs
  settings.rs
  memory.rs
  thread.rs
  hkask_mcp_media.rs
  hkask_mcp_kata_kanban.rs
  hkask_mcp_corpus.rs
  hkask_lisp.rs
)

is_scoped() {
  local b=${1##*/}
  local s
  for s in "${SCOPED_BASENAMES[@]}"; do
    [ "$b" = "$s" ] && return 0
  done
  return 1
}

# Resolve a cited path from the doc's context — the same ladder the
# existence gate uses (root, doc-relative, unique suffix, unique basename).
rsfiles="$(git ls-files '*.rs' | sort)"
resolve() {
  local doc=$1 p=$2 d b hits n bhits bn
  [ -f "$p" ] && { printf '%s\n' "$p"; return; }
  d=$(dirname "$doc")
  [ -f "$d/$p" ] && { printf '%s\n' "$d/$p"; return; }
  hits=$(grep -x ".*/$p" <<<"$rsfiles" || true)
  n=$(printf '%s\n' "$hits" | grep -c . || true)
  [ "$n" -eq 1 ] && { printf '%s\n' "$hits"; return; }
  b=${p##*/}
  bhits=$(grep -x ".*/$b" <<<"$rsfiles" || true)
  bn=$(printf '%s\n' "$bhits" | grep -c . || true)
  [ "$bn" -eq 1 ] && { printf '%s\n' "$bhits"; return; }
  return 1
}

mismatch=0; annotated=0; unannotated=0; unresolvable=0

mapfile -t DOCS < <(find "$DOCS_ROOT" -name '*.md' | sort)

for doc in "${DOCS[@]}"; do
  # Extract `path.rs:LINE@symbol` (annotation present) and `path.rs:LINE`
  # (bare) separately, so the two verdicts never mix.
  while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    cite=${hit%@*}; sym=${hit#*@}
    # The line is the FIRST number (a range's start) — `path.rs:548-746@sym`
    # annotates the definition enclosing line 548.
    l=$(printf '%s' "${cite##*:}" | sed 's/-.*//')
    p=${cite%:*}
    is_scoped "$p" || continue
    if ! r=$(resolve "$doc" "$p"); then
      unresolvable=$((unresolvable + 1))
      continue
    fi
    annotated=$((annotated + 1))
    actual=$(bash "$RESOLVER" "$r" "$l" 2>/dev/null | cut -f1)
    if [ -z "$actual" ]; then
      mismatch=$((mismatch + 1))
      echo "::error::citation-anchor: $doc cites $cite@$sym — no enclosing definition at $r:$l (line drifted past its symbol; re-anchor with kask/scripts/resolve-citation-anchor.sh)"
      continue
    fi
    # Match the full name or the "::" tail (a bare method name is accepted
    # for an impl-qualified def, so the common annotation stays short).
    tail=${actual##*::}
    if [ "$actual" != "$sym" ] && [ "$tail" != "$sym" ]; then
      mismatch=$((mismatch + 1))
      echo "::error::citation-anchor: $doc cites $cite@$sym — that line is inside '$actual' now (the citation rotted; re-anchor with kask/scripts/resolve-citation-anchor.sh)"
    fi
  done < <(grep -oE '[A-Za-z0-9_./-]+\.rs:[0-9]+(-[0-9]+)?@[A-Za-z_][A-Za-z0-9_:]*' "$doc" 2>/dev/null | sort -u)

  # Count unannotated citations into scoped files (the grandfathered set).
  while IFS= read -r cite; do
    [ -n "$cite" ] || continue
    p=${cite%:*}
    is_scoped "$p" || continue
    unannotated=$((unannotated + 1))
  done < <(grep -oE '[A-Za-z0-9_./-]+\.rs:[0-9]+' "$doc" 2>/dev/null | sort -u)
done

echo "----"
if [ "$mismatch" -gt 0 ]; then
  echo "docs citation anchors: $mismatch MISMATCHED (failures), $annotated annotated ok, $unannotated unannotated (scoped, grandfathered) over ${#DOCS[@]} docs"
  exit 1
fi
echo "docs citation anchors: $annotated annotated, all matched; $unannotated unannotated in scoped files (grandfathered — annotate when you touch a citation)"
exit 0
