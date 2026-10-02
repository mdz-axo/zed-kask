#!/usr/bin/env bash
# Self-test for the build-profile gate (build/check-build-profile.sh).
#
# Pins the gate's failure mode against a fixture tree (the gate's ROOT
# override): a root Cargo.toml that lost [profile.release-mcp] must fail
# with the D46 message, and a complete fixture must pass — including the
# clippy-wrapper exercise, which runs a copy of the real script/clippy
# against a stubbed cargo.
set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/build/check-build-profile.sh"
[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

repo="$KASK_ROOT/.."
mkdir -p "$fixture/.cargo" "$fixture/crates/zed" "$fixture/kask/scripts/build" "$fixture/script"
cp "$KASK_ROOT/scripts/build/install.sh" "$fixture/kask/scripts/build/install.sh"
cp "$repo/script/clippy" "$fixture/script/clippy"
cp "$repo/script/setup-sccache" "$fixture/script/setup-sccache"
cat > "$fixture/Cargo.toml" <<'EOF'
[profile.release]
lto = "thin"

[profile.release.package."*"]
codegen-units = 16

[profile.release.package.zed]
codegen-units = 16

[profile.release-mcp]
inherits = "release"
lto = false
codegen-units = 16
EOF
printf '[build]\njobs = 16\n' > "$fixture/.cargo/config.toml"
printf '[features]\n' > "$fixture/crates/zed/Cargo.toml"

failures=0

# Case 1: the profile is gone → the D46 failure.
sed -i '/^\[profile\.release-mcp\]/,$d' "$fixture/Cargo.toml"
set +e
out="$(ROOT="$fixture" bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q 'lost \[profile.release-mcp\]'; then
  echo "FAIL (case 1 — missing profile): rc=$rc"
  printf '%s\n' "$out" | tail -4
  failures=$((failures + 1))
else
  echo "OK (case 1 — missing profile): gate failed with the D46 message"
fi

# Case 2: the complete fixture passes (the clippy-wrapper exercise runs).
cat >> "$fixture/Cargo.toml" <<'EOF'

[profile.release-mcp]
inherits = "release"
lto = false
codegen-units = 16
EOF
set +e
out="$(ROOT="$fixture" bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
  echo "FAIL (case 2 — complete fixture): expected 0, got $rc"
  printf '%s\n' "$out" | tail -4
  failures=$((failures + 1))
else
  echo "OK (case 2 — complete fixture): gate exited 0"
fi

# Case 3: the --fast parity path is gone → the gate fails with its message.
cp "$KASK_ROOT/scripts/build/install.sh" "$fixture/kask/scripts/build/install.sh"
sed -i '/--profile release-fast/d' "$fixture/kask/scripts/build/install.sh"
set +e
out="$(ROOT="$fixture" bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q 'lost the --fast zed build path'; then
  echo "FAIL (case 3 — fast path removed): rc=$rc"
  printf '%s\n' "$out" | tail -4
  failures=$((failures + 1))
else
  echo "OK (case 3 — fast path removed): gate failed with the --fast message"
fi

# Case 4: the --debug cost warning is gone → the gate fails.
cp "$KASK_ROOT/scripts/build/install.sh" "$fixture/kask/scripts/build/install.sh"
sed -i '/debug_assertions/d' "$fixture/kask/scripts/build/install.sh"
set +e
out="$(ROOT="$fixture" bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q -- '--debug must warn'; then
  echo "FAIL (case 4 — debug warning removed): rc=$rc"
  printf '%s\n' "$out" | tail -4
  failures=$((failures + 1))
else
  echo "OK (case 4 — debug warning removed): gate failed with the warning-pin message"
fi

if [ "$failures" -eq 0 ]; then
  echo "SELFTEST OK: build-profile gate is alive (missing-profile, complete, fast-path, debug-warning all pinned)"
  exit 0
fi
echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
exit 1
