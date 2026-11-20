#!/usr/bin/env bash
# Shared selftest harness for the kask/scripts/check-*-selftest.sh gates.
#
# The duplication gates' selftests share the same synthetic-repo setup and
# pass/fail accounting. This file is the one implementation; each selftest
# sources it and supplies only its own cases. (P5.5: a new instance of an
# existing pattern is a signal to consolidate the pattern, not copy it —
# the ratchet selftest and the pin selftest previously cloned this harness.)
#
# Sourced, never executed: it defines selftest_init, expect and
# selftest_finish and fails loudly if run directly.

# shellcheck disable=SC2329  # expect() is invoked by the sourcing selftests
expect() { # expect <actual-exit> <expected-exit> <label>
  if [ "$1" -ne "$2" ]; then
    echo "FAIL: $3 (expected exit $2, got $1)"
    FAILURES=$((FAILURES + 1))
  else
    echo "ok: $3"
  fi
}

selftest_init() { # selftest_init <gate-basename>
  local gate_name="${1:?gate name required}"
  GATE="$(cd "$(dirname "$0")" && pwd)/$gate_name"
  [ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }
  WORK="$(mktemp -d)"
  trap 'rm -rf "$WORK"' EXIT
  FAILURES=0
}

selftest_finish() { # selftest_finish <ok-message>
  if [ "$FAILURES" -eq 0 ]; then
    echo ""
    echo "SELFTEST OK: $1"
    exit 0
  fi
  echo "" >&2
  echo "SELFTEST FAIL: $FAILURES case(s) did not behave as expected — the gate is vacuous or broken." >&2
  exit 1
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  echo "FAIL: $0 is a sourced harness, not an executable selftest." >&2
  exit 1
fi