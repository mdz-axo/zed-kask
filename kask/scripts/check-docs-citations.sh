#!/usr/bin/env bash
# Docs-citation gate (the tree-sitter adoption proposal's UC-3 cleanup,
# kask/docs/research/tree-sitter-adoption-proposal.md): every
# `path.rs:line` citation in kask/docs resolves to a live file in the
# current tree, via a resolution ladder — repo root, then doc-relative,
# then a unique repo-wide path-suffix match, then a unique basename
# match. A citation whose file cannot be resolved anywhere is rotted
# (the doc cites a deleted surface) and fails the gate.
#
# Line drift (the resolved file exists but the cited line moved) is a
# WARNING, not a failure: re-anchoring is the resolver's job
# (kask/scripts/resolve-citation-anchor.sh), and the doc-update pass
# consumes these warnings. Ambiguous bare names (a basename with several
# repo matches — the doc's own context picks the crate) are warnings
# too: the gate is the mechanical floor, not the reader.
#
# The 2026-10-11 lesson this gate encodes: a root-only resolution check
# reported "671/1656 citations rotted" — false absence, because docs
# cite crate-relative paths (`src/hkask_mcp_media.rs`) and bare names
# (`thread.rs:160`) that resolve from doc context. Measure with the
# ladder before reporting rot.
#
# Run locally: bash kask/scripts/check-docs-citations.sh
# Exit codes: 0 = every citation resolves (warnings allowed),
#             1 = rotted citations, 2 = usage/tooling.
set -uo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "FAIL: not inside a git repository (run from within the repo)." >&2
  exit 2
}
cd "$REPO_ROOT" || exit 2
DOCS_ROOT="${DOCS_ROOT:-kask/docs}"

command -v grep >/dev/null 2>&1 || {
  echo "FAIL: grep not found — the gate extracts citations with grep -oE." >&2
  exit 2
}

[ -d "$DOCS_ROOT" ] || {
  echo "FAIL: docs root $DOCS_ROOT not found." >&2
  exit 2
}

# Every .md under the docs root, tracked or not (new untracked docs are
# as citable as old ones).
mapfile -t DOCS < <(find "$DOCS_ROOT" -name '*.md' | sort)

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
git ls-files '*.rs' | sort > "$TMP/rsfiles.txt"

rotted=0; warn_line=0; warn_ambig=0; resolved=0

resolve() {
  # $1 doc, $2 path → prints the resolved repo-relative path or nothing.
  local doc=$1 p=$2 d b hits n bhits bn
  [ -f "$p" ] && { printf '%s\n' "$p"; return; }
  d=$(dirname "$doc")
  [ -f "$d/$p" ] && { printf '%s\n' "$d/$p"; return; }
  hits=$(grep -x ".*/$p" "$TMP/rsfiles.txt" || true)
  n=$(printf '%s\n' "$hits" | grep -c . || true)
  [ "$n" -eq 1 ] && { printf '%s\n' "$hits"; return; }
  b=${p##*/}
  bhits=$(grep -x ".*/$b" "$TMP/rsfiles.txt" || true)
  bn=$(printf '%s\n' "$bhits" | grep -c . || true)
  [ "$bn" -eq 1 ] && { printf '%s\n' "$bhits"; return; }
  return 1
}

for doc in "${DOCS[@]}"; do
  while IFS= read -r cite; do
    [ -n "$cite" ] || continue
    p=${cite%:*}; l=${cite##*:}
    if r=$(resolve "$doc" "$p"); then
      resolved=$((resolved + 1))
      maxl=$(wc -l < "$r")
      if ! [ "$l" -ge 1 ] 2>/dev/null || [ "$l" -gt "$maxl" ]; then
        warn_line=$((warn_line + 1))
        echo "warn: $doc: $cite resolves to $r but line $l is out of range (file has $maxl lines) — re-anchor with kask/scripts/resolve-citation-anchor.sh"
      fi
    else
      b=${p##*/}
      bhits=$(grep -x ".*/$b" "$TMP/rsfiles.txt" || true)
      bn=$(printf '%s\n' "$bhits" | grep -c . || true)
      if [ "$bn" -gt 1 ]; then
        warn_ambig=$((warn_ambig + 1))
        echo "warn: $doc: $cite — ambiguous bare name ($bn repo matches); resolve from doc context"
      else
        rotted=$((rotted + 1))
        echo "::error::docs-citation: $doc cites $cite — no live file resolves (deleted surface)"
      fi
    fi
  done < <(grep -oE '[A-Za-z0-9_./-]+\.rs:[0-9]+' "$doc" 2>/dev/null | sort -u)
done

echo "----"
echo "docs citations: $resolved resolved, $rotted rotted (failures), $warn_line line-drift warnings, $warn_ambig ambiguous-name warnings, over ${#DOCS[@]} docs"
[ "$rotted" -eq 0 ]