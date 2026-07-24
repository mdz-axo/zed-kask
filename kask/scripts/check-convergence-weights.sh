#!/usr/bin/env bash
# CI gate: convergence metric weights in .j2 templates must sum to ~1.0.
#
# Rationale: convergence-check.j2 templates define weighted dimensions
# (e.g., "weight: 0.40"). If the weights don't sum to 1.0, the convergence
# metric is mathematically incorrect — the skill will never converge (or
# always converges) regardless of the actual findings.
#
# This gate extracts weight values from lines containing "weight:" and
# verifies they sum to 1.0 ± 0.02 (floating-point tolerance).
#
# Exit codes:
#   0 — all convergence-check.j2 files have weights summing to ~1.0
#   1 — one or more files have weights that don't sum to ~1.0
#
# Usage: bash scripts/check-convergence-weights.sh

set -euo pipefail
cd "$(dirname "$0")/.."

FAIL=0
checked=0
skipped=0

for j2_file in registry/templates/*/convergence-check.j2; do
  [ -f "$j2_file" ] || continue
  checked=$((checked + 1))

  # Extract weight values from lines containing "weight:" followed by a number.
  # This matches patterns like:
  #   "Critical + high unresolved findings weight: 0.40"
  #   "Defense-layer coverage weight: 0.25"
  #   "weight: 0.40" (standalone)
  # It does NOT match scoring contributions like "+0.40" or "(0.40)".
  # NOTE: Do NOT sort -u — multiple dimensions can have the same weight (e.g., two 0.10 dimensions).
  weights=$(grep -E 'weight: 0\.[0-9]+' "$j2_file" 2>/dev/null \
    | grep -oE '0\.[0-9]+' \
    || true)

  if [ -z "$weights" ]; then
    # No weights found — skip (not all convergence-check.j2 files use weights)
    skipped=$((skipped + 1))
    continue
  fi

  # Sum the weights using awk (handles floating-point)
  sum=$(echo "$weights" | awk '{s+=$1} END {printf "%.4f", s}')

  # Check if sum is within [0.98, 1.02] (±0.02 tolerance)
  if (( $(echo "$sum > 1.02" | bc -l 2>/dev/null || echo 0) )) \
    || (( $(echo "$sum < 0.98" | bc -l 2>/dev/null || echo 0) )); then
    echo "::error::$j2_file: convergence weights sum to $sum (expected ~1.0)"
    echo "  weights found: $(echo "$weights" | tr '\n' ' ')"
    FAIL=1
  fi
done

if [ "$FAIL" -eq 0 ]; then
  echo "OK: convergence weights sum to ~1.0 ($checked checked, $skipped skipped — no weights found)."
  exit 0
else
  echo ""
  echo "FAIL: convergence metric weights don't sum to ~1.0."
  echo "Fix: adjust the weight values so they sum to exactly 1.0."
  exit 1
fi
