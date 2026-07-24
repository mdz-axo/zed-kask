#!/usr/bin/env bash
# CI gate: every MCP server must have tool-behavior contract tests that invoke
# tools through their public `Parameters<T>` seam.
#
# Rationale: the hkask-mcp-filesystem review found three shipped logic bugs
# (slice-index panics on bad input, canonicalize-on-non-existent, silent
# no-ops) that had ZERO `unwrap()` calls and were invisible to a panic-grep and
# to helper-seam-only tests. Only tool-behavior contract tests (calling tools
# via `Parameters<T>`) catch this class. See:
#   docs/reference/mcp-servers/README.md  (Testing standard)
#   docs/status/mcp-fleet-test-seam-audit-2026-07-17.md
#
# This gate is RATCHETED: servers not yet covered are listed in ALLOWLIST below.
# As each server gains a tool-behavior test, remove it from ALLOWLIST. When
# ALLOWLIST is empty, the standard is fully enforced and cannot regress.
#
# Limitation: the gate keys on the literal `Parameters(` token, a heuristic —
# a helper that happens to use `Parameters(` would satisfy the gate without a
# real tool-behavior test (false positive). It is a ratchet, not a proof; rely
# on review for genuine tool-behavior coverage. Tighten only if it false-passes
# in practice.
#
# Exit codes:
#   0 — all servers either have tool-behavior tests or are allowlisted
#   1 — a server lacks tool-behavior tests AND is not allowlisted (regression)
#
# Usage: bash scripts/check-mcp-tool-tests.sh
#        HKASK_MCP_TOOL_TEST_STRICT=1  # treat allowlisted gaps as warnings only
#                                     # (still exit 0) — use during ramp-up

set -euo pipefail

# Servers known to lack tool-behavior contract tests today. Shrink over time.
# Remove a name the moment its tests/ contains a `Parameters(` call.
ALLOWLIST=(
)

# Servers that are EXEMPT by design (not agent-facing tool surfaces requiring
# contract tests). Add only with a documented reason.
EXEMPT=()

is_listed() {
  local name="$1"
  local item
  for item in "${ALLOWLIST[@]}"; do
    [ "$item" = "$name" ] && return 0
  done
  for item in "${EXEMPT[@]}"; do
    [ "$item" = "$name" ] && return 0
  done
  return 1
}

violations=0
ratchet_gaps=0

for server_dir in mcp-servers/hkask-mcp-*/; do
  [ -d "$server_dir" ] || continue
  name="$(basename "$server_dir")"
  tests_dir="${server_dir}tests"

  has_tool_tests=0
  if [ -d "$tests_dir" ]; then
    # A tool-behavior test calls a tool method through Parameters<T>.
    if grep -rIlE "Parameters\(" "$tests_dir" --include='*.rs' >/dev/null 2>&1; then
      has_tool_tests=1
    fi
  fi

  if [ "$has_tool_tests" -eq 1 ]; then
    continue
  fi

  # No tool-behavior tests found.
  if is_listed "$name"; then
    ratchet_gaps=$((ratchet_gaps + 1))
    echo "ratchet: $name lacks tool-behavior tests (allowlisted — ${#ALLOWLIST[@]} remaining)"
  else
    violations=$((violations + 1))
    echo "::error::MCP server '$name' has no tool-behavior contract tests (no 'Parameters(' in ${tests_dir}). Add tests via the public tool seam, or add to ALLOWLIST with a reason. See docs/reference/mcp-servers/README.md §Testing standard."
  fi
done

echo "summary: $violations violation(s), $ratchet_gaps allowlisted gap(s), ${#ALLOWLIST[@]} in ratchet allowlist"

if [ "${HKASK_MCP_TOOL_TEST_STRICT:-0}" = "1" ]; then
  # Ramp-up mode: allowlisted gaps are warnings only.
  if [ "$violations" -gt 0 ]; then
    exit 1
  fi
  exit 0
fi

# Default: allowlisted gaps are tolerated (ratchet), violations fail.
[ "$violations" -eq 0 ]