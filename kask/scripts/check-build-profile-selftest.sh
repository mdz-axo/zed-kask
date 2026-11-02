#!/usr/bin/env bash
# Self-test for the build-profile gate (build/check-build-profile.sh).
#
# Pins the gate's failure mode against a fixture tree (the gate's ROOT
# override): a root Cargo.toml that lost [profile.release-mcp] must fail
# with the D46 message, and a complete fixture must pass — including the
# clippy-wrapper exercise, which runs a copy of the real script/clippy
# against a stubbed cargo. Each DWARF-split setting (release-mcp and
# release-fast, D46 2026-11-02) is pinned by its own violation case: the
# gate must fail with its specific message when either profile loses
# split-debuginfo = "unpacked".
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

[profile.release-fast]
inherits = "release"
debug = "full"
split-debuginfo = "unpacked"
lto = false
codegen-units = 16

[profile.release-mcp]
inherits = "release"
lto = false
codegen-units = 16
split-debuginfo = "unpacked"
EOF
printf '[build]\njobs = 16\n' > "$fixture/.cargo/config.toml"
printf '[features]\n' > "$fixture/crates/zed/Cargo.toml"

# The complete [profile.release-mcp] block, restored wherever a case tore
# the section out — one canonical heredoc so every case builds on the same
# complete profile. release-fast sits before release-mcp in the fixture,
# so the delete-to-EOF below leaves it standing.
restore_release_mcp() {
  sed -i '/^\[profile\.release-mcp\]/,$d' "$fixture/Cargo.toml"
  cat >> "$fixture/Cargo.toml" <<'EOF'

[profile.release-mcp]
inherits = "release"
lto = false
codegen-units = 16
split-debuginfo = "unpacked"
EOF
}

failures=0

# Case 1: the profile is gone → the D46 failure.
restore_release_mcp
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
restore_release_mcp
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

# Case 3: release-mcp loses split-debuginfo → the D46 server-binary
# failure (the ~1.6 GB unstripped-servers message). The gate checks this
# before any install.sh check, so install.sh state is irrelevant here.
restore_release_mcp
sed -i '/^\[profile\.release-mcp\]/,$ { /^split-debuginfo = "unpacked"$/d }' "$fixture/Cargo.toml"
set +e
out="$(ROOT="$fixture" bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q 'limited DWARF ships'; then
  echo "FAIL (case 3 — release-mcp split removed): rc=$rc"
  printf '%s\n' "$out" | tail -4
  failures=$((failures + 1))
else
  echo "OK (case 3 — release-mcp split removed): gate failed with the release-mcp split message"
fi

# Case 4: release-fast loses split-debuginfo → the D46 --fast failure (the
# ~7 GB binary message). release-mcp must be complete again first — the
# gate checks it earlier. The range sed removes only release-fast's split
# line: the range ends at release-fast's own `lto = false`, before
# release-mcp's block is ever reached.
restore_release_mcp
sed -i '/^\[profile\.release-fast\]/,/^lto = false$/ { /^split-debuginfo = "unpacked"$/d }' "$fixture/Cargo.toml"
set +e
out="$(ROOT="$fixture" bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q 'full DWARF links'; then
  echo "FAIL (case 4 — release-fast split removed): rc=$rc"
  printf '%s\n' "$out" | tail -4
  failures=$((failures + 1))
else
  echo "OK (case 4 — release-fast split removed): gate failed with the release-fast split message"
fi

# Case 5: the --fast parity path is gone → the gate fails with its message.
cp "$KASK_ROOT/scripts/build/install.sh" "$fixture/kask/scripts/build/install.sh"
sed -i '/--profile release-fast/d' "$fixture/kask/scripts/build/install.sh"
set +e
out="$(ROOT="$fixture" bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q 'lost the --fast zed build path'; then
  echo "FAIL (case 5 — fast path removed): rc=$rc"
  printf '%s\n' "$out" | tail -4
  failures=$((failures + 1))
else
  echo "OK (case 5 — fast path removed): gate failed with the --fast message"
fi

# Case 6: a dev-profile build route returns → the gate fails.
cp "$KASK_ROOT/scripts/build/install.sh" "$fixture/kask/scripts/build/install.sh"
printf '\nHKASK_BUILD_TYPE="debug"\n' >> "$fixture/kask/scripts/build/install.sh"
set +e
out="$(ROOT="$fixture" bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q 'dev-profile build route'; then
  echo "FAIL (case 6 — dev-profile route returns): rc=$rc"
  printf '%s\n' "$out" | tail -4
  failures=$((failures + 1))
else
  echo "OK (case 6 — dev-profile route returns): gate failed with the removal message"
fi

if [ "$failures" -eq 0 ]; then
  echo "SELFTEST OK: build-profile gate is alive (missing-profile, complete, release-mcp split, release-fast split, fast-path, no-dev-profile all pinned)"
  exit 0
fi
echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
exit 1