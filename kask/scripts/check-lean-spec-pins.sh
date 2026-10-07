#!/usr/bin/env bash
# Lean spec pins gate: every positive pin in kask/lean checks clean under the
# pinned toolchain (exit 0, no `sorry` in the output), and every negative
# control FAILS — a checker that accepts a negative control is not an
# oracle. The gate also fails when either class is empty: a corpus with no
# positive pins checks nothing, and a corpus with no negative controls has
# no oracle proof.
#
# The toolchain is provisioned by the shared install_lean_toolchain
# (build/install-common.sh): elan v4.2.4 + the pinned
# leanprover/lean4:v4.34.0, checksum-gated, cached in $ELAN_HOME (default
# ~/.elan). The pins are core-Lean only (no Mathlib), so per-file `lean`
# needs no lake environment.
#
# Overrides (used by check-lean-spec-pins-selftest.sh, which drives this
# gate offline over fixture corpora with a fake elan/lean):
#   LEAN_SPEC_DIR  the corpus directory (default: ../lean from this script)
#   ELAN_HOME      the elan installation (install_lean_toolchain's own default)
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
source "$here/build/install-common.sh"

lean_dir="${LEAN_SPEC_DIR:-$here/../lean}"
[ -d "$lean_dir" ] || { echo "FAIL: LEAN_SPEC_DIR is not a directory: $lean_dir"; exit 1; }

install_lean_toolchain
elan_home="${ELAN_HOME:-${HOME:?HOME is required}/.elan}"
elan_bin="$elan_home/bin/elan"

# Run `lean` from the pinned toolchain over one file.
lean_check_file() {
    ELAN_HOME="$elan_home" "$elan_bin" run "$LEAN_CHECK_TOOLCHAIN" lean "$1"
}

positives=0
negatives=0
fail=0
for f in "$lean_dir"/*.lean; do
    [ -e "$f" ] || { echo "FAIL: no .lean files in $lean_dir"; exit 1; }
    name=$(basename "$f")
    if [[ "$name" == *negative_control* ]]; then
        negatives=$((negatives + 1))
        if lean_check_file "$f" >/dev/null 2>&1; then
            echo "FAIL: negative control $name checks clean — it must fail (a checker that accepts it is not an oracle)"
            fail=1
        else
            echo "ok: negative control $name fails as required"
        fi
    else
        positives=$((positives + 1))
        out=$(lean_check_file "$f" 2>&1) || {
            echo "FAIL: positive pin $name does not check:"
            printf '%s\n' "$out"
            fail=1
            continue
        }
        if [[ "$out" == *sorry* ]]; then
            echo "FAIL: positive pin $name carries a proof hole (sorry):"
            printf '%s\n' "$out"
            fail=1
        else
            echo "ok: positive pin $name checks clean"
        fi
    fi
done
[ "$positives" -gt 0 ] || { echo "FAIL: no positive spec pins found in $lean_dir — the gate is vacuous"; exit 1; }
[ "$negatives" -gt 0 ] || { echo "FAIL: no negative controls found in $lean_dir — the oracle check is vacuous"; exit 1; }

if [ "$fail" -eq 0 ]; then
    echo "Lean spec pins: $positives positive pin(s) clean, $negatives negative control(s) failing, under $LEAN_CHECK_TOOLCHAIN"
fi
exit "$fail"