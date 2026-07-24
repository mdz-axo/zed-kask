#!/usr/bin/env bash
# Conformance check: superforecasting skill ↔ hkask-forecast Rust primitives.
#
# Asserts bidirectional consistency between the "Deterministic Primitives"
# contract table in registry/templates/superforecasting/README.md and the
# public methodology primitives in crates/hkask-forecast/src/lib.rs:
#   1. Every #[must_use]-tagged pub fn in hkask-forecast is named in the
#      contract table (no orphan primitives).
#   2. Every function the contract table names exists in hkask-forecast
#      (no dangling references).
#
# The #[must_use] filter selects the Tetlock methodology primitives
# (Fermi averaging, shrinkage, Bayes, Brier) and excludes struct constructors
# like `FermiQuestion::new`, which are not methodology stages.
#
# Enabled in CI via .github/workflows/ci.yml invariants job.
# Run locally: scripts/check-forecast-conformance.sh

set -euo pipefail
cd "$(dirname "$0")/.."

LIB="crates/hkask-forecast/src/lib.rs"
CONTRACT="registry/templates/superforecasting/README.md"
SECTION="Deterministic Primitives"

[ -f "$LIB" ] || { echo "FAIL: $LIB not found"; exit 1; }
[ -f "$CONTRACT" ] || { echo "FAIL: $CONTRACT not found"; exit 1; }

grep -q "## $SECTION" "$CONTRACT" || {
    echo "FAIL: '$SECTION' contract section not found in $CONTRACT"
    exit 1
}

FAIL=0

# Methodology primitives in hkask-forecast: pub fn immediately preceded by #[must_use].
mapfile -t PUB_FNS < <(
    grep -A1 '^#\[must_use' "$LIB" \
        | grep -oE '^pub fn [a-z_]+\(' \
        | sed -E 's/^pub fn ([a-z_]+)\(/\1/' \
        | sort -u
)

# Function names referenced in the contract table's function column only.
# The table is fenced between "## Deterministic Primitives" and the next "## ".
# Field 3 of a "|"-delimited row is the function column; Notes (field 4) is
# excluded so heuristic names like `triage_question` (which live in MCP servers,
# not hkask-forecast) are not mistaken for canonical primitives.
mapfile -t CONTRACT_FNS < <(
    awk '/^## '"$SECTION"'/{f=1;next} f && /^## /{exit} f' "$CONTRACT" \
        | awk -F'|' '/^\|/ {print $3}' \
        | grep -oE '`[a-z_]+`' \
        | tr -d '`' \
        | sort -u
)

if [ "${#PUB_FNS[@]}" -eq 0 ]; then
    echo "FAIL: no #[must_use] pub fns parsed from $LIB — parsing broke."
    exit 1
fi

# 1. Orphans: primitives in the lib not named in the contract.
ORPHANS=()
for fn in "${PUB_FNS[@]}"; do
    if ! printf '%s\n' "${CONTRACT_FNS[@]}" | grep -qx "$fn"; then
        ORPHANS+=("$fn")
    fi
done

# 2. Dangles: contract references that do not exist in the lib.
DANGLES=()
for fn in "${CONTRACT_FNS[@]}"; do
    if ! printf '%s\n' "${PUB_FNS[@]}" | grep -qx "$fn"; then
        DANGLES+=("$fn")
    fi
done

if [ ${#ORPHANS[@]} -gt 0 ]; then
    echo "FAIL: hkask-forecast primitives not in conformance contract:"
    for fn in "${ORPHANS[@]}"; do
        echo "  - $fn"
    done
    echo "Add each to the '$SECTION' table in $CONTRACT."
    FAIL=1
fi

if [ ${#DANGLES[@]} -gt 0 ]; then
    echo "FAIL: contract references functions not in hkask-forecast:"
    for fn in "${DANGLES[@]}"; do
        echo "  - $fn"
    done
    echo "Implement in $LIB or correct the contract table."
    FAIL=1
fi

if [ $FAIL -eq 0 ]; then
    echo "OK: superforecasting conformance contract aligned with hkask-forecast."
    echo "  ${#PUB_FNS[@]} primitives, ${#CONTRACT_FNS[@]} contract references."
    exit 0
else
    exit 1
fi