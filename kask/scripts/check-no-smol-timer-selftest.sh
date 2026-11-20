#!/usr/bin/env bash
# Self-test for the smol-timer gate (check-no-smol-timer.sh): a fixture
# with a smol::Timer call fails naming it; a clean fixture passes; a scan
# error (grep rc 2) fails loudly, never as a silent pass.
set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-no-smol-timer.sh"
[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/widget/src"

failures=0

# Case 1: a smol::Timer call fails, named.
printf 'fn x() { smol::Timer::after(std::time::Duration::from_secs(1)); }\n' > "$fixture/widget/src/lib.rs"
set +e; out="$(SCAN_ROOTS="$fixture/widget" bash "$GATE" 2>&1)"; rc=$?; set -e
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q "widget/src/lib.rs"; then
  echo "FAIL (case 1 — smol::Timer detected): rc=$rc"; printf '%s\n' "$out" | tail -4; failures=$((failures + 1))
else
  echo "OK (case 1 — smol::Timer detected and named)"
fi

# Case 2: the clean fixture passes.
printf 'fn x() {}\n' > "$fixture/widget/src/lib.rs"
set +e; out="$(SCAN_ROOTS="$fixture/widget" bash "$GATE" 2>&1)"; rc=$?; set -e
if [ "$rc" -ne 0 ]; then
  echo "FAIL (case 2 — clean fixture): expected 0, got $rc"; printf '%s\n' "$out" | tail -4; failures=$((failures + 1))
else
  echo "OK (case 2 — clean fixture passes)"
fi

# Case 3: an unscannable root fails loudly (grep rc 2), never a silent pass.
set +e; out="$(SCAN_ROOTS="$fixture/does-not-exist" bash "$GATE" 2>&1)"; rc=$?; set -e
if [ "$rc" -eq 0 ]; then
  echo "FAIL (case 3 — scan error): gate exited 0 on an unscannable root"; failures=$((failures + 1))
else
  echo "OK (case 3 — scan error fails loudly)"
fi

if [ "$failures" -eq 0 ]; then
  echo "SELFTEST OK: smol-timer gate is alive (violation + clean + scan-error all pinned)"
  exit 0
fi
echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
exit 1
