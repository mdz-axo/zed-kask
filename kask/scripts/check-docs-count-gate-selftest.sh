#!/usr/bin/env bash
# Self-test for the docs-count gate (check-docs-count-gate.sh): a consistent
# fixture passes; a cap breach, a stale README measurement, an unparsable
# policy sentence, a divergent gate reference, and an unscannable root each
# fail naming the cause; target-exceedance alone never fails (the target is
# the condensation direction, not a gate).
set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-docs-count-gate.sh"
[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

# write_fixture CAP TARGET RECORDED EXTRA [README_CAP]
# Base tree: README.md + DOCUMENTATION_STANDARDS.md + policy.md = 3 files;
# EXTRA adds that many more. README_CAP (default CAP) lets the README's gate
# references diverge from the canonical policy on purpose.
write_fixture() {
    local cap=$1 target=$2 recorded=$3 extra=$4 readme_cap=${5:-$1}
    rm -rf "$fixture/docs"
    mkdir -p "$fixture/docs/architecture"
    printf '**Corpus size (measured 2026-09-30):** %s files total (`find kask/docs -type f | wc -l`) — under the formal **fewer-than-%s** count gate, with a working **%s-file target** as the condensation direction.\n' \
        "$recorded" "$readme_cap" "$target" > "$fixture/docs/README.md"
    printf -- '- [x] Document count is %s, under the fewer-than-%s cap with a working %s-file target.\n' \
        "$recorded" "$readme_cap" "$target" >> "$fixture/docs/README.md"
    printf '**Condensation requirement:** The docs tree is capped at **fewer than %s files** (all files), with a working **target of %s files** on the same all-files basis.\n' \
        "$cap" "$target" > "$fixture/docs/architecture/DOCUMENTATION_STANDARDS.md"
    printf -- '- [ ] Document-count gate: the tree holds fewer than %s files; working target: %s files\n' \
        "$cap" "$target" >> "$fixture/docs/architecture/DOCUMENTATION_STANDARDS.md"
    printf 'find kask/docs -type f | wc -l     # must stay < %s (working target: %s)\n' \
        "$cap" "$target" >> "$fixture/docs/architecture/DOCUMENTATION_STANDARDS.md"
    touch "$fixture/docs/architecture/policy.md"
    local i
    for ((i = 0; i < extra; i++)); do
        touch "$fixture/docs/extra-$i.md"
    done
}

run_gate() {
    DOCS_ROOT="$fixture/docs" \
    DOCS_STANDARDS="$fixture/docs/architecture/DOCUMENTATION_STANDARDS.md" \
    DOCS_README="$fixture/docs/README.md" \
    bash "$GATE" 2>&1
}

failures=0
expect_fail() { # NAME EXPECTED_SUBSTRING OUTPUT RC
    local name=$1 needle=$2 out=$3 rc=$4
    if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q "$needle"; then
        echo "FAIL ($name): rc=$rc, expected failure naming '$needle'"
        printf '%s\n' "$out" | tail -4
        failures=$((failures + 1))
    else
        echo "OK ($name)"
    fi
}
expect_pass() { # NAME EXPECTED_SUBSTRING OUTPUT RC
    local name=$1 needle=$2 out=$3 rc=$4
    if [ "$rc" -ne 0 ] || ! printf '%s\n' "$out" | grep -q "$needle"; then
        echo "FAIL ($name): rc=$rc, expected pass reporting '$needle'"
        printf '%s\n' "$out" | tail -4
        failures=$((failures + 1))
    else
        echo "OK ($name)"
    fi
}

# Case 1: the consistent fixture passes.
write_fixture 10 5 3 0
set +e; out="$(run_gate)"; rc=$?; set -e
expect_pass "clean fixture" "3 files under the fewer-than-10 cap" "$out" "$rc"

# Case 2: a cap breach fails naming the cap.
write_fixture 2 1 3 0
set +e; out="$(run_gate)"; rc=$?; set -e
expect_fail "cap breach" "at or over the fewer-than-2 cap" "$out" "$rc"

# Case 3: a stale README measurement fails naming both numbers.
write_fixture 10 5 7 0
set +e; out="$(run_gate)"; rc=$?; set -e
expect_fail "stale README measurement" "records 7 files total but the tree holds 3" "$out" "$rc"

# Case 4: an unparsable policy sentence fails loudly, never a silent pass.
write_fixture 10 5 3 0
printf 'The docs tree should stay small.\n' > "$fixture/docs/architecture/DOCUMENTATION_STANDARDS.md"
set +e; out="$(run_gate)"; rc=$?; set -e
expect_fail "unparsable policy" "cannot parse the cap" "$out" "$rc"

# Case 5: a gate reference that lags the canonical cap fails naming it.
write_fixture 10 5 3 0 9
set +e; out="$(run_gate)"; rc=$?; set -e
expect_fail "reference divergence" "does not carry the canonical cap (10)" "$out" "$rc"

# Case 6: target-exceedance alone never fails — the target is direction.
write_fixture 10 2 3 0
set +e; out="$(run_gate)"; rc=$?; set -e
expect_pass "target note is non-failing" "exceeds the 2-file working target" "$out" "$rc"

# Case 7: an unscannable root fails loudly, never as a silent pass.
write_fixture 10 5 3 0
set +e; out="$(DOCS_ROOT="$fixture/does-not-exist" DOCS_STANDARDS="$fixture/docs/architecture/DOCUMENTATION_STANDARDS.md" DOCS_README="$fixture/docs/README.md" bash "$GATE" 2>&1)"; rc=$?; set -e
expect_fail "unscannable root" "docs root not found" "$out" "$rc"

if [ "$failures" -eq 0 ]; then
    echo "SELFTEST OK: docs-count gate is alive (clean + breach + stale + unparsable + divergence + target-note + unscannable all pinned)"
    exit 0
fi
echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
exit 1
