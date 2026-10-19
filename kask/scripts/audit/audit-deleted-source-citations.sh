#!/usr/bin/env bash
# Audit whether each removed corpus source's recorded citation sites have
# actually left the tree.
#
# Why this exists: the zk-ref-open recompose (2026-10-18) removed sources by
# the criterion "every citation site has left the tree". That criterion is
# recorded in `sources/MANIFEST.json` as per-entry `citation_sites` (path:line).
# Line numbers rot as the tree moves, so a citation site can survive as a
# live path whose cited line no longer mentions the source — making the
# removal unverifiable from the record alone. This instrument reports every
# deleted entry that still names an existing file, so the removal criterion
# can be re-checked after any tree shift.
#
# A listed entry is a REVIEW CANDIDATE, not a mis-removal: the cited path may
# merely have drifted, or may cite a *different* surviving sibling source.
# Verify the cited line's content before concluding a removal was wrong.
#
# Usage: audit-deleted-source-citations.sh [manifest.json] [repo-root]
#   defaults: ~/Documents/zk-data/zk-reference-open/sources/MANIFEST.json
#             ~/Clones/zed-kask
# Exit 0 always (a report, not a gate); findings are the printed lines.
set -euo pipefail

MANIFEST="${1:-$HOME/Documents/zk-data/zk-reference-open/sources/MANIFEST.json}"
ROOT="${2:-$HOME/Clones/zed-kask}"

if [ ! -f "$MANIFEST" ]; then
  echo "manifest not found: $MANIFEST" >&2
  exit 2
fi
if [ ! -d "$ROOT" ]; then
  echo "repo root not found: $ROOT" >&2
  exit 2
fi

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

# tab-separated: slug <TAB> space-separated citation sites
jq -r '.entries[] | select(.status=="deleted") | [.slug, ((.citation_sites // []) | join(" "))] | @tsv' \
  "$MANIFEST" > "$tmp"

total=$(wc -l < "$tmp" | tr -d ' ')
echo "deleted entries: $total"
echo "review candidates (a recorded citation site still exists):"
echo

flagged=0
while IFS= read -r line; do
  slug="${line%%$'\t'*}"
  sites="${line#*$'\t'}"
  [ -z "$sites" ] && continue
  live=""
  for s in $sites; do
    f="${s%%:*}"
    if [ -f "$ROOT/$f" ]; then
      live="$live $f"
    fi
  done
  if [ -n "$live" ]; then
    printf '  %s ->%s\n' "$slug" "$live"
    flagged=$((flagged + 1))
  fi
done < "$tmp"

echo
echo "flagged: $flagged of $total (each is a review candidate — verify the cited line's content)"
