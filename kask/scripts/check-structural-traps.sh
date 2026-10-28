#!/usr/bin/env bash
# CI gate: no net-new structural trap instances beyond the committed
# baseline (UC-2 of the tree-sitter adoption proposal,
# kask/docs/research/tree-sitter-adoption-proposal.md).
#
# The .rules trap "No `unwrap()` in production code" had no dedicated
# enforcement: a textual grep cannot exclude `#[cfg(test)]` modules,
# string literals, or comments, so it either false-positives or is not
# written at all. The ast-grep rule (kask/ast-grep/rules/
# no-unwrap-in-production.yml) matches the call structurally and excludes
# test modules by syntax, not text.
#
# Ratchet: the baseline is kask/scripts/structural-traps-baseline.txt —
# one `path:line` entry per current instance (the dead-code-allow-
# baseline.txt format). Every current instance must appear in the
# baseline; a net-new instance fails CI. Removals never fail — they leave
# the baseline stale-high, refreshable any time via --refresh.
#
# Scope: the check-unsafe-forbid.sh surface — every kask crate
# (kask/crates/*, kask/mcp-servers/*) plus the zed-path kask-owned crates
# (crates/hkask-*, crates/swarm_panel, crates/marketplace_ui_common),
# src/ trees only (tests/ dirs are test code: "in test code prefer
# expect with a message"). Inline `#[cfg(test)]` mods are excluded by
# the rule itself.
#
# SCAN_DIRS and BASELINE_FILE are overridable so the self-test
# (check-structural-traps-selftest.sh) can point the gate at a temp tree.
#
# Run locally: bash kask/scripts/check-structural-traps.sh
# Refresh baseline: bash kask/scripts/check-structural-traps.sh --refresh
# Exit codes: 0 = no net-new instances, 1 = net-new instances or
# tooling unavailable.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null)" || {
  echo "FAIL: $SCRIPT_DIR is not inside a git repository." >&2
  exit 1
}
cd "$REPO_ROOT" || exit 1

BASELINE_FILE="${BASELINE_FILE:-$SCRIPT_DIR/structural-traps-baseline.txt}"
AST_GREP="$SCRIPT_DIR/ast-grep.sh"
RULE_FILE="$SCRIPT_DIR/../ast-grep/rules/no-unwrap-in-production.yml"

command -v jq >/dev/null 2>&1 || {
  echo "FAIL: jq not found — the gate parses ast-grep's stream JSON with jq." >&2
  exit 1
}
[ -x "$AST_GREP" ] || {
  echo "FAIL: $AST_GREP missing or not executable." >&2
  exit 1
}
[ -f "$RULE_FILE" ] || {
  echo "FAIL: rule file $RULE_FILE missing." >&2
  exit 1
}

if [ -n "${SCAN_DIRS+x}" ]; then
  # shellcheck disable=SC2206
  scan_dirs=($SCAN_DIRS)
else
  scan_dirs=(
    kask/crates/*/src
    kask/mcp-servers/*/src
    crates/hkask-*/src
    crates/swarm_panel/src
    crates/marketplace_ui_common/src
  )
fi

paths=()
for d in "${scan_dirs[@]}"; do
  [ -d "$d" ] && paths+=("$d")
done
if [ "${#paths[@]}" -eq 0 ]; then
  echo "OK: no scan dirs match — nothing to check (the 0 = 0 path the unsafe-forbid selftest pins)."
  exit 0
fi

# Stream JSON: one match object per line on stdout; the error-severity
# summary goes to stderr and the exit code is 1 when matches exist —
# both expected, hence the redirect and || true. Grep reads from the temp
# file, never a pipe (a pipe under pipefail SIGPIPEs the writer).
CURRENT="$(mktemp)"
trap 'rm -f "$CURRENT"' EXIT
"$AST_GREP" scan --rule "$RULE_FILE" --json=stream "${paths[@]}" 2>/dev/null \
  | jq -r 'select(.ruleId == "no-unwrap-in-production") | "\(.file):\(.range.start.line + 1)"' \
  | sort -u > "$CURRENT" || true

if [ "${1:-}" = "--refresh" ]; then
  mkdir -p "$(dirname "$BASELINE_FILE")"
  sort -u "$CURRENT" > "$BASELINE_FILE"
  echo "baseline refreshed: $(grep -c . "$BASELINE_FILE" || true) instance(s) in $BASELINE_FILE"
  exit 0
fi

[ -f "$BASELINE_FILE" ] || {
  echo "FAIL: baseline $BASELINE_FILE missing — run with --refresh to record the current instances." >&2
  exit 1
}

current_count=$(grep -c . "$CURRENT" || true)
net_new=0
while IFS= read -r c; do
  [ -n "$c" ] || continue
  if ! grep -qxF -- "$c" "$BASELINE_FILE"; then
    echo "::error::structural-trap: $c — net-new unwrap() in production code beyond $BASELINE_FILE"
    net_new=$((net_new + 1))
  fi
done < "$CURRENT"

stale=0
while IFS= read -r b; do
  [ -n "$b" ] || continue
  if ! grep -qxF -- "$b" "$CURRENT"; then
    stale=$((stale + 1))
  fi
done < "$BASELINE_FILE"

echo "----"
echo "structural traps: $current_count current instance(s), $net_new net-new, $stale stale (removals never fail; refresh any time)"
[ "$net_new" -eq 0 ]