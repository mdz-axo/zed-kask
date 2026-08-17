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
#
# Each entry is `name|added-date|reason`. The date is a real field (not a
# comment) so the staleness check below can parse it. Entries older than
# STALE_DAYS emit a non-failing warning in CI output — making a stalled
# ratchet visible without forcing a deadline that may not be resourced.
# (Follow-up issue #3: the quota prevents growth but doesn't force shrinkage;
# visibility is the smallest honest fix.)
STALE_DAYS=${STALE_DAYS:-90}
ALLOWLIST_MAX=9
ALLOWLIST=(
  "hkask-mcp-codegraph|2026-07-17|tools query a code-graph DB; need a populated graph fixture"
  "hkask-mcp-companies|2026-07-17|tools require SerpAPI/external HTTP; need network mocking"
  "hkask-mcp-condenser|2026-07-17|tool invokes an LLM condenser; need an inference mock"
  "hkask-mcp-corpus|2026-07-17|tools require SQLite + embedding store; need a fixture store"
  "hkask-mcp-media|2026-07-17|tools call Fal.ai workflow APIs; need a media-API mock"
  "hkask-mcp-portfolio|2026-07-17|no tests dir yet; tools wrap portfolio storage"
  "hkask-mcp-prediction-markets|2026-07-17|tools fetch live Polymarket/Kalshi data; need network mocking"
  "hkask-mcp-swarm|2026-07-17|existing tests use the panel invoke seam / live HTTP, not Parameters<T>"
  "hkask-mcp-training|2026-07-17|tools require inference + HF Hub; need mocks"
)

# Servers that are EXEMPT by design (not agent-facing tool surfaces requiring
# contract tests). Add only with a documented reason.
EXEMPT=()

is_listed() {
  local name="$1"
  local item entry_name
  for item in "${ALLOWLIST[@]}"; do
    entry_name="${item%%|*}"
    [ "$entry_name" = "$name" ] && return 0
  done
  for item in "${EXEMPT[@]}"; do
    entry_name="${item%%|*}"
    [ "$entry_name" = "$name" ] && return 0
  done
  return 1
}

# Extract the date field (field 2) from a `name|date|reason` entry.
entry_date() {
  local entry="$1"
  printf '%s' "$entry" | awk -F'|' '{print $2}'
}

# Days between a YYYY-MM-DD date and today (UTC). Uses date(1); GNU date on
# ubuntu-24.04 runners. Returns 0 on any parse failure (fail-open for age —
# staleness is a warning, not a failure, so a parse glitch must not break CI).
days_since() {
  local then="$1"
  local then_epoch now_epoch
  then_epoch=$(date -u -d "$then" +%s 2>/dev/null) || { echo 0; return; }
  now_epoch=$(date -u +%s 2>/dev/null) || { echo 0; return; }
  echo $(( (now_epoch - then_epoch) / 86400 ))
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

echo "summary: $violations violation(s), $ratchet_gaps allowlisted gap(s), ${#ALLOWLIST[@]} in ratchet allowlist (cap: $ALLOWLIST_MAX)"

# Stale-entry visibility (follow-up issue #3, option 1): warn — do not fail —
# for allowlisted entries older than STALE_DAYS. This makes a stalled ratchet
# visible in CI output without forcing a deadline that may not be resourced.
# A gate that can't fail is a gate that doesn't enforce, but a ratchet that
# can't be seen to stall is a ratchet that won't be acted on.
stale_warnings=0
for entry in "${ALLOWLIST[@]}"; do
  name="${entry%%|*}"
  added=$(entry_date "$entry")
  if [ -z "$added" ]; then
    echo "::warning::$name has no added-date field — add one so staleness is trackable."
    stale_warnings=$((stale_warnings + 1))
    continue
  fi
  age=$(days_since "$added")
  if [ "$age" -gt "$STALE_DAYS" ]; then
    echo "::warning::$name has been allowlisted for ${age} days (added $added, stale threshold $STALE_DAYS). Consider adding a tool-behavior test or documenting why the deferral persists."
    stale_warnings=$((stale_warnings + 1))
  fi
done
if [ "$stale_warnings" -gt 0 ]; then
  echo "staleness: $stale_warnings entr(y/ies) above the $STALE_DAYS-day visibility threshold (warnings, not failures)"
fi

# Ratchet quota: fail if the allowlist grew beyond the high-water mark.
# Adding a server requires removing one — or explicitly bumping ALLOWLIST_MAX
# with a documented reason. This converts the one-way clutch (no backsliding)
# into a true ratchet (forced forward progress).
if [ "${#ALLOWLIST[@]}" -gt "$ALLOWLIST_MAX" ]; then
  echo "::error::MCP tool-test allowlist grew to ${#ALLOWLIST[@]} (cap: $ALLOWLIST_MAX). "
  echo "  Add a tool-behavior test to an existing allowlisted server and remove it,"
  echo "  or bump ALLOWLIST_MAX in scripts/check-mcp-tool-tests.sh with a justification."
  exit 1
fi

if [ "${HKASK_MCP_TOOL_TEST_STRICT:-0}" = "1" ]; then
  # Ramp-up mode: allowlisted gaps are warnings only.
  if [ "$violations" -gt 0 ]; then
    exit 1
  fi
  exit 0
fi

# Default: allowlisted gaps are tolerated (ratchet), violations fail.
[ "$violations" -eq 0 ]