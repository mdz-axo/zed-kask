#!/usr/bin/env bash
# check-reg-creep.sh — defend against REG namespace creep.
#
# Scans all tracing::target: "reg.*" strings in crates/ and mcp-servers/ and
# verifies each is registered in CANONICAL_NAMESPACES by EXACT match (not the
# hierarchical ancestor prefix used by check-reg-canonical.sh).
#
# This catches ad-hoc sub-namespaces that would otherwise silently validate
# via the hierarchical is_canonical rule — e.g. `reg.meta.self_calibration.typo`
# passes check-reg-canonical.sh (ancestor `reg.meta.self_calibration` is
# canonical) but fails here unless the exact string is registered.
#
# Complementary to check-reg-canonical.sh (which is the CI-enforced gate):
#   - check-reg-canonical.sh: hierarchical ancestor match, CI-enforced
#   - check-reg-creep.sh:     exact match, local pre-commit anti-creep guard
#
# Usage: scripts/check-reg-creep.sh
# Exit: 0 = all targets registered (exact match), 1 = unregistered targets found

set -euo pipefail
cd "$(dirname "$0")/.."

# Scan roots for `target: "reg.*"` strings. Overridable so the self-test
# (check-reg-creep-selftest.sh) can point the gate at a temp tree with a
# synthetic violation. Defaults to the production source roots.
SCAN_DIRS=( ${SCAN_DIRS:-crates/ mcp-servers/} )
REGISTRY="${REGISTRY:-crates/hkask-types/src/event.rs}"

[ -f "$REGISTRY" ] || { echo "FAIL: registry not found at $REGISTRY" >&2; exit 1; }
for d in "${SCAN_DIRS[@]}"; do
    [ -d "$d" ] || { echo "FAIL: scan directory not found: $d" >&2; exit 1; }
done

# Extract canonical namespaces strictly from CANONICAL_NAMESPACES (never from
# comments or negative unit tests elsewhere in event.rs).
canonical=$(awk '/^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?const[[:space:]]+CANONICAL_NAMESPACES([^A-Za-z0-9_]|$)/{f=1;next} f&&/^[[:space:]]*\];/{f=0} f' "$REGISTRY" \
    | { grep -oE '"reg\.[^"]*"' || true; } \
    | sed 's/"//g' \
    | sort -u)
if [ -z "$canonical" ]; then
    echo "FAIL: parsed zero canonical namespaces from CANONICAL_NAMESPACES in $REGISTRY" >&2
    exit 1
fi

# Extract all reg.* tracing targets from Rust source files.
targets=$( { grep -roh --include='*.rs' --exclude-dir=target 'target: "reg\.[^"]*"' "${SCAN_DIRS[@]}" || [ $? -eq 1 ]; } \
    | sed 's/target: "//;s/"//' \
    | { grep -v '()' || true; } \
    | sort -u )

if [ -z "$targets" ]; then
    echo "check-reg-creep: no reg.* targets found"
    exit 0
fi

unregistered=0
for target in $targets; do
    if ! echo "$canonical" | grep -qx "$target"; then
        echo "check-reg-creep: UNREGISTERED target '$target' (exact match — ancestor may be canonical, see check-reg-canonical.sh)"
        unregistered=$((unregistered + 1))
    fi
done

if [ "$unregistered" -gt 0 ]; then
    echo "check-reg-creep: $unregistered unregistered reg.* target(s) found (exact match)"
    echo "Either register the exact namespace in CANONICAL_NAMESPACES"
    echo "(crates/hkask-types/src/event.rs) or retarget the span to hkask.*"
    exit 1
fi

echo "check-reg-creep: all reg.* targets registered (exact match)"