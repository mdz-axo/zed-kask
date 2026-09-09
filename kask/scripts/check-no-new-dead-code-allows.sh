#!/usr/bin/env bash
# CI gate: no net-new #[allow(dead_code)] beyond the committed baseline.
#
# Every strangler-fig residue finding from the 2026-09-08 audit carried an
# #[allow(dead_code)] — it is the mechanism that hides partial removals and
# write-only surface from the compiler (a struct-level allow even masked
# never-read fields). This gate fails when any tracked .rs file contains
# MORE allow(dead_code) occurrences than the baseline records.
#
# Adding an allow therefore requires a visible, reviewable act: bumping
# scripts/dead-code-allow-baseline.txt in the same commit, with the
# justification in the code where the next agent reads it. Removals never
# fail the gate — they leave the baseline stale-high, refreshable anytime.
#
# This is a fork of upstream Zed: the baseline deliberately includes
# upstream's own allows (crates/gpui, crates/project, ...). An upstream
# merge that brings in new upstream allows trips the gate once; refreshing
# the baseline in the merge commit is part of reviewing what upstream
# brought in.
#
# Run locally: bash kask/scripts/check-no-new-dead-code-allows.sh
# Refresh baseline (after legitimate removals or an upstream merge):
#   bash kask/scripts/check-no-new-dead-code-allows.sh --refresh
# Exit codes: 0 = no file exceeds its baseline, 1 = violations found
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null)" || {
  echo "FAIL: $SCRIPT_DIR is not inside a git repository."
  exit 1
}
cd "$REPO_ROOT" || exit 1

BASELINE="$SCRIPT_DIR/dead-code-allow-baseline.txt"

# Per-file counts of allow(dead_code) across ALL tracked .rs files (the
# whole fork — crates/ and kask/), `path:count` per line, zero-count files
# omitted. -H forces the filename prefix even when a batch contains a
# single file (bare counts would corrupt the parse); -r skips the run
# entirely on empty input (grep with no file argument would read stdin).
current_counts() {
  git ls-files '*.rs' \
    | xargs -r grep -cH 'allow(dead_code)' 2>/dev/null \
    | grep -v ':0$' \
    | sort
}

if [ "${1:-}" = "--refresh" ]; then
  current_counts > "$BASELINE"
  echo "OK: baseline refreshed — $(wc -l < "$BASELINE") files carry allow(dead_code), $(awk -F: '{s+=$2} END {print s+0}' "$BASELINE") total occurrences. Commit it with the change that justifies the delta."
  exit 0
fi

if [ ! -f "$BASELINE" ]; then
  echo "FAIL: baseline $BASELINE missing — run '$0 --refresh' and commit it alongside the gate."
  exit 1
fi

CURRENT=$(mktemp)
trap 'rm -f "$CURRENT"' EXIT
current_counts > "$CURRENT"

VIOLATIONS=0
while IFS= read -r line; do
  file="${line%%:*}"
  count="${line##*:}"
  base=$(awk -F: -v f="$file" '$1 == f {print $2; exit}' "$BASELINE")
  base="${base:-0}"
  if [ "$count" -gt "$base" ]; then
    echo "FAIL: $file carries $count allow(dead_code) — baseline $base."
    echo "  Justify the addition in the code, then bump the baseline in the same commit:"
    echo "  bash kask/scripts/check-no-new-dead-code-allows.sh --refresh"
    VIOLATIONS=1
  fi
done < "$CURRENT"

if [ "$VIOLATIONS" -eq 0 ]; then
  echo "OK: no file exceeds its allow(dead_code) baseline."
fi
exit "$VIOLATIONS"
