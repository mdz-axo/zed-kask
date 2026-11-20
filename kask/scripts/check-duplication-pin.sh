#!/usr/bin/env bash
# CI gate: the duplication scan-base pin must name the second parent of the
# most recent upstream merge.
#
# The pin (kask/scripts/duplication-scan-base.txt) defines the fork-diverged
# scan set the duplication ratchet measures against. A stale pin silently
# widens that set — upstream files enter the baseline — and the ratchet then
# fails with an unexplained overage (observed 2026-11-09: the pin named the
# 2026-10-12 merge while the 2026-11-08 merge was the last one; the scan set
# widened from 263 to 723 paths and the count read 6344 against a 5666
# baseline).
#
# The ratchet's own doctrine (check-duplication-ratchet.sh): "Refresh the
# pin in the upstream-merge commit". This gate makes that doctrine
# mechanical: it fails the moment a merge lands without refreshing the pin.
#
# Exit codes: 0 = pin current, 1 = pin stale/missing/unverifiable.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null)" || {
  echo "FAIL: $SCRIPT_DIR is not inside a git repository."
  exit 1
}
cd "$REPO_ROOT" || exit 1

PIN_FILE="$SCRIPT_DIR/duplication-scan-base.txt"
[ -f "$PIN_FILE" ] || {
  echo "FAIL: $PIN_FILE missing — pin the upstream merge-base commit and commit it with the gate."
  exit 1
}
PIN="$(head -n1 "$PIN_FILE" | tr -d '[:space:]')"
[ -n "$PIN" ] || {
  echo "FAIL: $PIN_FILE is empty."
  exit 1
}

MERGE="$(git log --merges -1 --format=%H 2>/dev/null)"
[ -n "$MERGE" ] || {
  echo "FAIL: no merge commit found on this branch — cannot verify the pin."
  exit 1
}
SECOND="$(git rev-parse --verify -q "${MERGE}^2" 2>/dev/null)"
[ -n "$SECOND" ] || {
  echo "FAIL: the most recent merge $MERGE has no second parent."
  exit 1
}

if [ "$PIN" != "$SECOND" ]; then
  echo "FAIL: duplication scan-base pin is stale."
  echo "  pin:      $PIN"
  echo "  expected: $SECOND (second parent of $MERGE)"
  echo "  Refresh it in the upstream-merge commit, together with duplication-ratchet-baseline.txt."
  exit 1
fi

echo "OK: duplication scan-base pin is current ($PIN)."