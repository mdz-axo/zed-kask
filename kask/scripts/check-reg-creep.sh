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

# Extract all reg.* tracing targets from Rust source files.
# Matches: target: "reg.foo.bar" (with optional whitespace)
# Filters out false positives like reg.config() (method calls, not namespaces).
# `|| true` guards the pipeline under `set -euo pipefail` — grep returns 1 when
# no matches survive the final filter, which must NOT abort the script.
targets=$( { grep -roh 'target: "reg\.[^"]*"' crates/ mcp-servers/ \
    | sed 's/target: "//;s/"//' \
    | grep -v '()' \
    | sort -u; } || true )

if [ -z "$targets" ]; then
    echo "check-reg-creep: no reg.* targets found"
    exit 0
fi

# Check each target against CANONICAL_NAMESPACES in event.rs.
# EXACT match — unlike check-reg-canonical.sh, no ancestor trimming.
canonical=$(grep -o '"reg\.[^"]*"' crates/hkask-types/src/event.rs \
    | sed 's/"//g' \
    | sort -u)

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