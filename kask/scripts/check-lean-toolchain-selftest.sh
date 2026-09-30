#!/usr/bin/env bash
# Self-test for the lean-toolchain gate (build/check-lean-toolchain.sh).
#
# The gate is itself an offline contract test of the shared installer; this
# pins its failure mode: a fixture copy of the build scripts whose
# install.sh lost its guarded `install_lean_toolchain || return 1` call
# sites must fail the gate's call-site count check, and the unmodified
# copies must pass.
set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
src="$KASK_ROOT/scripts/build"
[ -f "$src/check-lean-toolchain.sh" ] || { echo "FAIL: gate not found at $src/check-lean-toolchain.sh"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
cp "$src/check-lean-toolchain.sh" "$src/install-common.sh" "$src/install-binary.sh" "$src/install.sh" "$src/mcp-servers.txt" "$fixture/"

failures=0

# Case 1: the unmodified copies pass (the offline contract holds).
set +e
out="$(bash "$fixture/check-lean-toolchain.sh" 2>&1)"
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
  echo "FAIL (case 1 — unmodified copies): expected 0, got $rc"
  printf '%s\n' "$out" | tail -4
  failures=$((failures + 1))
else
  echo "OK (case 1 — unmodified copies): gate exited 0"
fi

# Case 2: install.sh lost its guarded call sites → the count check fails.
sed -i '/install_lean_toolchain || return 1/d' "$fixture/install.sh"
set +e
out="$(bash "$fixture/check-lean-toolchain.sh" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ]; then
  echo "FAIL (case 2 — broken call sites): gate exited 0 on an install.sh with no guarded Lean provisioning"
  failures=$((failures + 1))
else
  echo "OK (case 2 — broken call sites): gate failed on the missing call sites"
fi

if [ "$failures" -eq 0 ]; then
  echo "SELFTEST OK: lean-toolchain gate is alive (contract + call-site break both pinned)"
  exit 0
fi
echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
exit 1
