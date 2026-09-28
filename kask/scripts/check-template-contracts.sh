#!/usr/bin/env bash
# CI gate: every registry .j2 contract agrees with its body.
#
# Wraps the advisory instrument kask/scripts/audit/skill-corpus-contract-audit.sh
# (unused declared input, absent declared output, undeclared consumption)
# and turns any flag into a failing exit. `contract.output` has no runtime
# reader — the caller↔template handoff is documented, not validated — so
# this gate is the only place a declared output the body never produces is
# caught (operator decision 2026-09-28). Run locally:
#   bash kask/scripts/check-template-contracts.sh
set -euo pipefail

repo_root="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
audit="$repo_root/kask/scripts/audit/skill-corpus-contract-audit.sh"
[ -f "$audit" ] || { echo "FAIL: missing $audit" >&2; exit 1; }

output="$(bash "$audit")"
summary="$(printf '%s\n' "$output" | tail -n 1)"
flagged="$(printf '%s\n' "$summary" | sed -nE 's/^checked [0-9]+ templates; ([0-9]+) flagged;.*$/\1/p')"
if [ -z "$flagged" ]; then
  echo "FAIL: contract audit produced no summary line:" >&2
  printf '%s\n' "$output" >&2
  exit 1
fi
if [ "$flagged" -ne 0 ]; then
  echo "FAIL: $flagged template(s) whose [inference] contract disagrees with the body:" >&2
  printf '%s\n' "$output" | grep -E '^  ' >&2 || true
  echo "Fix the contract or the body; a declared output the body never produces is a broken handoff." >&2
  exit 1
fi
echo "OK: $summary"
