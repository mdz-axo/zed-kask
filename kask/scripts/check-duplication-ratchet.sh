#!/usr/bin/env bash
# CI gate: no net-new duplication clusters beyond the committed baseline.
#
# Simplification program, Phase 1.2 (mission 2026-10-01): a token-similarity
# scan (jscpd, min-tokens 70) over kask/ and the fork-diverged files; the
# cluster count lives in duplication-ratchet-baseline.txt and new clusters
# fail CI. The instance class this makes mechanical: the redraw grid existed
# as two private copies before consolidation.
#
# Scan set: kask/ plus every file that differs from the upstream commit
# pinned in duplication-scan-base.txt (the second parent of the last
# upstream merge), filtered to source extensions. The pin — not a live
# upstream ref — defines the set, so a bare CI clone (no upstream remote,
# depth 1 plus one fetched commit) computes the same set as a full clone.
# Refresh the pin in the upstream-merge commit, the same doctrine as the
# dead-code baseline: refreshing is part of reviewing what upstream brought.
#
# Adding duplication therefore requires a visible, reviewable act: bumping
# duplication-ratchet-baseline.txt in the same commit, with the justification
# where the next agent reads it. Removals never fail the gate — they leave
# the baseline stale-high, refreshable anytime.
#
# Run locally: bash kask/scripts/check-duplication-ratchet.sh
# Refresh baseline: bash kask/scripts/check-duplication-ratchet.sh --refresh
# Exit codes: 0 = at or below baseline, 1 = new clusters / tooling unavailable
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null)" || {
  echo "FAIL: $SCRIPT_DIR is not inside a git repository."
  exit 1
}
cd "$REPO_ROOT" || exit 1

BASELINE="$SCRIPT_DIR/duplication-ratchet-baseline.txt"
SCAN_BASE_FILE="$SCRIPT_DIR/duplication-scan-base.txt"
JSCPD_VERSION=5.4.0
MIN_TOKENS=70
# jscpd 5.x takes language names, not extensions (tsx/jsx are separate
# tokens); the scan-set grep below pre-filters to the matching extensions.
FORMATS='rust,typescript,tsx,javascript,jsx,bash'

command -v npx >/dev/null 2>&1 || {
  echo "FAIL: npx not found — the duplication ratchet requires node/npx (jscpd $JSCPD_VERSION)."
  exit 1
}
command -v jq >/dev/null 2>&1 || {
  echo "FAIL: jq not found — the duplication ratchet parses jscpd's JSON report with jq."
  exit 1
}

[ -f "$SCAN_BASE_FILE" ] || {
  echo "FAIL: $SCAN_BASE_FILE missing — pin the upstream merge-base commit and commit it with the gate."
  exit 1
}
SCAN_BASE="$(head -n1 "$SCAN_BASE_FILE")"
git cat-file -e "$SCAN_BASE^{commit}" 2>/dev/null || {
  echo "FAIL: scan base '$SCAN_BASE' (from $SCAN_BASE_FILE) is not a commit in this repository — refresh the pin in the upstream-merge commit."
  exit 1
}

# kask/ (all fork surface) + fork-diverged files outside kask/, source
# extensions only. Written to a file so the space guard reads the same list
# the scanner consumes.
scan_set() {
  {
    echo kask
    git diff --name-only --diff-filter=ACMR "$SCAN_BASE"..HEAD \
      | grep -E '\.(rs|ts|tsx|js|jsx|sh)$' \
      | grep -v '^kask/'
  }
}

count_clusters() {
  local out list report rc
  out="$(mktemp -d)" || return 1
  list="$out/scan-paths.txt"
  scan_set > "$list"
  if grep -q ' ' "$list"; then
    echo "FAIL: a scan-set path contains a space — refusing to pass a corrupted set to the scanner." >&2
    rm -rf "$out"
    return 1
  fi
  # The scan reads the shared working tree: other agent streams edit this
  # repo concurrently, and their in-flight changes move the count between
  # scans (observed live 2026-10-01: a concurrent stream adding a mirrored
  # trait to hkask-storage's driver.rs raised the count by one
  # mid-session). The comparison below re-verifies any below-baseline
  # reading for exactly this reason.
  # shellcheck disable=SC2046
  if ! npx --yes "jscpd@$JSCPD_VERSION" \
      --min-tokens "$MIN_TOKENS" \
      --format "$FORMATS" \
      --reporters json \
      --output "$out" \
      --fail-on-empty \
      $(cat "$list") >/dev/null 2>"$out/stderr.log"; then
    echo "FAIL: jscpd scan did not complete — a broken scan must fail visibly, never read as zero clusters:" >&2
    cat "$out/stderr.log" >&2
    rm -rf "$out"
    return 1
  fi
  report="$(find "$out" -name 'jscpd-report.json' -print -quit)"
  if [ -z "$report" ]; then
    echo "FAIL: jscpd wrote no JSON report." >&2
    rm -rf "$out"
    return 1
  fi
  jq '.statistics.total.clones' "$report"
  rc=$?
  rm -rf "$out"
  return "$rc"
}

if [ "${1:-}" = "--refresh" ]; then
  count="$(count_clusters)" || exit 1
  {
    echo "$count"
    echo "# Duplication-cluster ratchet (simplification program, Phase 1.2)."
    echo "# Metric: jscpd $JSCPD_VERSION --min-tokens $MIN_TOKENS statistics.total.clones"
    echo "# over kask/ + fork-diverged files vs the scan base pinned in"
    echo "# duplication-scan-base.txt. The scan reads the shared working tree."
    echo "# Bump in the same commit as the addition that justifies it; refresh after"
    echo "# removals and in the upstream-merge commit (together with the scan-base pin)."
  } > "$BASELINE"
  echo "OK: baseline refreshed — $count duplication clusters. Commit it with the change that justifies the delta."
  exit 0
fi

[ -f "$BASELINE" ] || {
  echo "FAIL: baseline $BASELINE missing — run '$0 --refresh' and commit it alongside the gate."
  exit 1
}
BASE_COUNT="$(grep -m1 -E '^[0-9]+$' "$BASELINE")"
if [ -z "${BASE_COUNT:-}" ]; then
  echo "FAIL: no cluster count found in $BASELINE."
  exit 1
fi

CURRENT="$(count_clusters)" || exit 1

if [ "$CURRENT" -gt "$BASE_COUNT" ]; then
  echo "FAIL: $CURRENT duplication clusters exceed the baseline $BASE_COUNT."
  echo "  Deduplicate, or justify the addition and bump the baseline in the same commit:"
  echo "  bash kask/scripts/check-duplication-ratchet.sh --refresh"
  exit 1
fi

if [ "$CURRENT" -lt "$BASE_COUNT" ]; then
  # A below-baseline reading is only tightenable if a re-scan reproduces
  # it: the scan reads the shared working tree, and a concurrent edit
  # (or a scanner hiccup) must never lure the baseline down.
  CONFIRM="$(count_clusters)" || exit 1
  if [ "$CONFIRM" -eq "$CURRENT" ]; then
    echo "OK: $CURRENT duplication clusters (baseline $BASE_COUNT — the ratchet can tighten; refresh with --refresh in the commit that earned it)."
  else
    echo "OK: $CURRENT duplication clusters (baseline $BASE_COUNT) — measurement instability: re-scan read $CONFIRM. jscpd's known one-clone flap; not a tightenable reading, baseline unchanged."
  fi
else
  echo "OK: $CURRENT duplication clusters, at baseline."
fi
exit 0
