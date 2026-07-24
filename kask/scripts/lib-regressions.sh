#!/usr/bin/env bash
# Shared library for regression-check CI gates.
#
# Provides check_regressions() — a parameterized function that reads
# security/regressions/RR-*.yaml entries, filters by surface, and
# enforces grep-kind regressions against the codebase.
#
# Source this file from a wrapper script:
#
#   source scripts/lib-regressions.sh
#   check_regressions "training" "--include=*.py --include=*.yaml"
#
# Exit codes:
#   0 — all enforced grep regressions pass
#   1 — an enforced grep regression's pattern was found

# check_regressions <surface> <include_patterns> <deferred_kind_name>
#
# - surface: "" (all) or "training" / "supply-chain" / "runtime" / etc.
# - include_patterns: grep --include flags as a single string
# - deferred_kind_name: name for deferred regressions (e.g., "runtime-assert" / "reg-span")
check_regressions() {
  local surface="$1"
  local include_patterns="$2"
  local deferred_kind="$3"

  local REGRESSIONS_DIR="security/regressions"

  if [ ! -d "$REGRESSIONS_DIR" ]; then
    echo "OK: no regressions directory — nothing to check."
    return 0
  fi

  local violations=0
  local pending=0
  local enforced=0
  local deferred=0

  # Parse include_patterns into array for grep.
  local include_array=()
  if [ -n "$include_patterns" ]; then
    # shellcheck disable=SC2206
    include_array=($include_patterns)
  fi

  for rr_file in "$REGRESSIONS_DIR"/RR-*.yaml; do
    [ -f "$rr_file" ] || continue

    # Extract surface — skip if filtering and doesn't match.
    local rr_surface
    rr_surface=$(grep -m1 '^surface:' "$rr_file" | sed 's/^surface:\s*//')
    if [ -n "$surface" ] && [ "$rr_surface" != "$surface" ]; then
      continue
    fi

    # Extract fields (lightweight grep-based parsing — no yq dependency).
    local rr_id rr_status rr_kind rr_pattern rr_title
    rr_id=$(grep -m1 '^id:' "$rr_file" | sed 's/^id:\s*//')
    rr_status=$(grep -m1 '^status:' "$rr_file" | sed 's/^status:\s*//')
    rr_kind=$(grep -m1 'kind:' "$rr_file" | sed 's/.*kind:\s*//')
    rr_pattern=$(grep -m1 'pattern:' "$rr_file" | sed 's/.*pattern:\s*//' | sed 's/^"\(.*\)"$/\1/')
    rr_title=$(grep -m1 '^title:' "$rr_file" | sed 's/^title:\s*//' | sed 's/^"\(.*\)"$/\1/')

    # Skip kinds we don't handle.
    if [ "$rr_kind" != "grep" ] && [ "$rr_kind" != "$deferred_kind" ]; then
      continue
    fi

    # Deferred-kind regressions require runtime infrastructure not available in CI.
    # Acknowledge for visibility but don't enforce.
    if [ "$rr_kind" = "$deferred_kind" ]; then
      if [ "$rr_status" = "enforced" ]; then
        deferred=$((deferred + 1))
        echo "deferred: $rr_id is $deferred_kind (enforced but runtime check not in CI) — $rr_title"
      elif [ "$rr_status" = "pending" ]; then
        pending=$((pending + 1))
        echo "ratchet: $rr_id is pending (known, not yet enforced) — $rr_title"
      fi
      continue
    fi

    # grep-kind regressions are mechanically enforced.
    if [ "$rr_status" = "enforced" ]; then
      enforced=$((enforced + 1))
      local matches
      if [ ${#include_array[@]} -gt 0 ]; then
        matches=$(grep -rPn ${include_array[@]} "$rr_pattern" . \
          --exclude-dir=target --exclude-dir=.git --exclude-dir=node_modules \
          --exclude-dir=regressions \
          2>/dev/null || true)
      else
        # Fall back to per-regression include field (kali-style).
        local rr_include
        rr_include=$(grep -m1 'include:' "$rr_file" | sed 's/.*include:\s*//' | sed 's/^"\(.*\)"$/\1/')
        matches=$(grep -rPn "$rr_pattern" $rr_include 2>/dev/null || true)
      fi
      if [ -n "$matches" ]; then
        echo "::error::Regression $rr_id violated: $rr_title"
        echo "  pattern: $rr_pattern"
        echo "$matches" | head -5 | sed 's/^/    /'
        violations=$((violations + 1))
      fi
    elif [ "$rr_status" = "pending" ]; then
      pending=$((pending + 1))
      echo "ratchet: $rr_id is pending (known, not yet enforced) — $rr_title"
    fi
  done

  echo "summary: $violations violation(s), $enforced enforced, $pending pending, $deferred $deferred_kind (deferred)"

  [ "$violations" -eq 0 ]
}
