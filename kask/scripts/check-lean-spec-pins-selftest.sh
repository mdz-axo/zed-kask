#!/usr/bin/env bash
# Self-test for the Lean spec-pins gate (check-lean-spec-pins.sh).
#
# The gate needs a real Lean toolchain, so this selftest drives it offline
# over fixture corpora with a fake elan dispatching to a fake lean (the
# check-lean-toolchain.sh pattern): the fake lean classifies fixture files
# by marker, proving the gate fails on every synthetic violation class and
# passes on a good corpus. A gate that cannot fail is not an oracle.
#
# Violation classes pinned:
#   1. a positive pin that does not check        → FAIL, names the file
#   2. a positive pin carrying a sorry            → FAIL, names the hole
#   3. a negative control that checks clean      → FAIL, names the oracle
#   4. a corpus with no negative controls         → FAIL, names the vacuity
#   5. a corpus with no positive pins            → FAIL, names the vacuity
#   6. an empty corpus                            → FAIL, names the emptiness
#   7. a good corpus (clean positive + failing negative) → exit 0
set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
gate="$KASK_ROOT/scripts/check-lean-spec-pins.sh"
[ -f "$gate" ] || { echo "FAIL: gate not found at $gate"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

# Fake elan: toolchain install succeeds; `run <pinned> lean` dispatches to the
# fake lean (install_lean_toolchain's own --version calls included); `run
# <pinned> lake` reports a version. Anything else fails closed.
mkdir -p "$fixture/elan/bin" "$fixture/fake"
export ELAN_HOME="$fixture/elan"
cat > "$fixture/elan/bin/elan" <<'ELAN'
#!/bin/bash
case "$1 $2 $3" in
    'toolchain install leanprover/lean4:v4.34.0') exit 0 ;;
esac
if [ "$1" = run ] && [ "$2" = leanprover/lean4:v4.34.0 ]; then
    shift 2
    tool="$1"
    shift
    case "$tool" in
        lean)
            if [ "${1:-}" = --version ]; then
                printf '%s\n' 'Lean (version 4.34.0, test, Release)'
                exit 0
            fi
            exec "$FAKE_LEAN" "$@" ;;
        *) printf '%s\n' 'Lake version test (Lean version 4.34.0)'; exit 0 ;;
    esac
fi
exit 1
ELAN
chmod 700 "$fixture/elan/bin/elan"

# Fake lean: classify fixture files by marker.
#   GATE_FIXTURE_FAIL  → exit 1 (a positive carrying it must fail the gate)
#   GATE_FIXTURE_SORRY → exit 0 with a sorry warning (must fail the gate)
#   no marker          → exit 0 silently (a clean positive; a negative
#                        control carrying it must fail the gate)
cat > "$fixture/fake/lean" <<'LEAN'
#!/bin/bash
file="${1:-}"
[ -f "$file" ] || exit 1
if grep -q GATE_FIXTURE_FAIL "$file"; then exit 1; fi
if grep -q GATE_FIXTURE_SORRY "$file"; then
    printf '%s\n' "-- warning: $file uses 'sorry'" >&2
fi
exit 0
LEAN
chmod 700 "$fixture/fake/lean"
export FAKE_LEAN="$fixture/fake/lean"

failures=0
run_case() { # run_case <label> <dir> <expect_zero> <must_mention>
    local label="$1" dir="$2" expect_zero="$3" must_mention="${4:-}"
    local out rc
    set +e
    out="$(LEAN_SPEC_DIR="$dir" bash "$gate" 2>&1)"
    rc=$?
    set -e
    local ok=1
    if [ "$expect_zero" = yes ] && [ "$rc" -ne 0 ]; then ok=0; fi
    if [ "$expect_zero" = no ] && [ "$rc" -eq 0 ]; then ok=0; fi
    if [ -n "$must_mention" ] && ! printf '%s' "$out" | grep -q "$must_mention"; then ok=0; fi
    if [ "$ok" -eq 1 ]; then
        echo "OK ($label)"
    else
        echo "FAIL ($label): rc=$rc expect_zero=$expect_zero mention='$must_mention'"
        printf '%s\n' "$out" | tail -6
        failures=$((failures + 1))
    fi
}

# Case 7 first (the good corpus) — then every violation class.
good="$fixture/lean-good";      mkdir -p "$good"
printf 'clean positive\n' > "$good/pos.lean"
printf 'GATE_FIXTURE_FAIL\n' > "$good/neg_negative_control.lean"
run_case "good corpus passes" "$good" yes

bad_pos="$fixture/lean-bad-pos"; mkdir -p "$bad_pos"
printf 'GATE_FIXTURE_FAIL\n' > "$bad_pos/pos.lean"
printf 'GATE_FIXTURE_FAIL\n' > "$bad_pos/neg_negative_control.lean"
run_case "positive that does not check" "$bad_pos" no "does not check"

sorry_pos="$fixture/lean-sorry"; mkdir -p "$sorry_pos"
printf 'GATE_FIXTURE_SORRY\n' > "$sorry_pos/pos.lean"
printf 'GATE_FIXTURE_FAIL\n' > "$sorry_pos/neg_negative_control.lean"
run_case "positive carrying a sorry" "$sorry_pos" no "sorry"

passing_neg="$fixture/lean-passing-neg"; mkdir -p "$passing_neg"
printf 'clean positive\n' > "$passing_neg/pos.lean"
printf 'clean negative control — must fail\n' > "$passing_neg/neg_negative_control.lean"
run_case "negative control that checks clean" "$passing_neg" no "not an oracle"

no_neg="$fixture/lean-no-neg"; mkdir -p "$no_neg"
printf 'clean positive\n' > "$no_neg/pos.lean"
run_case "corpus with no negative controls" "$no_neg" no "no negative controls"

no_pos="$fixture/lean-no-pos"; mkdir -p "$no_pos"
printf 'GATE_FIXTURE_FAIL\n' > "$no_pos/neg_negative_control.lean"
run_case "corpus with no positive pins" "$no_pos" no "no positive spec pins"

empty="$fixture/lean-empty"; mkdir -p "$empty"
run_case "empty corpus" "$empty" no "no .lean files"

if [ "$failures" -eq 0 ]; then
    echo "SELFTEST OK: lean spec-pins gate is alive (good corpus + all six violation classes pinned)"
    exit 0
fi
echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
exit 1