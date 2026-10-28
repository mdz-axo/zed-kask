#!/usr/bin/env bash
# Self-test for the mod.rs gate (check-no-mod-rs.sh): a fixture tree with a
# mod.rs fails naming it; a clean fixture passes; an unscannable root fails
# loudly, never as a silent pass.
set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-no-mod-rs.sh"
[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/crates/sample/src/inner"

failures=0

# Case 1: a mod.rs on the surface fails, named.
touch "$fixture/crates/sample/src/inner/mod.rs"
set +e; out="$(SCAN_ROOTS="$fixture/crates" bash "$GATE" 2>&1)"; rc=$?; set -e
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q "inner/mod.rs"; then
  echo "FAIL (case 1 — mod.rs detected): rc=$rc"; printf '%s\n' "$out" | tail -4; failures=$((failures + 1))
else
  echo "OK (case 1 — mod.rs detected and named)"
fi

# Case 2: the clean fixture passes.
rm "$fixture/crates/sample/src/inner/mod.rs"
set +e; out="$(SCAN_ROOTS="$fixture/crates" bash "$GATE" 2>&1)"; rc=$?; set -e
if [ "$rc" -ne 0 ]; then
  echo "FAIL (case 2 — clean fixture): expected 0, got $rc"; printf '%s\n' "$out" | tail -4; failures=$((failures + 1))
else
  echo "OK (case 2 — clean fixture passes)"
fi

# Case 3: an unscannable root fails loudly, never a silent pass.
set +e; out="$(SCAN_ROOTS="$fixture/does-not-exist" bash "$GATE" 2>&1)"; rc=$?; set -e
if [ "$rc" -eq 0 ]; then
  echo "FAIL (case 3 — scan error): gate exited 0 on an unscannable root"; failures=$((failures + 1))
else
  echo "OK (case 3 — scan error fails loudly)"
fi

if [ "$failures" -eq 0 ]; then
  echo "SELFTEST OK: mod.rs gate is alive (violation + clean + scan-error all pinned)"
  exit 0
fi
echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
exit 1
