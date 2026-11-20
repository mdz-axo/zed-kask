#!/usr/bin/env bash
# Self-test for the workflow's cargo-fmt step (kask-invariants.yml,
# "Formatting — cargo fmt --check"). There is no check-fmt.sh gate script —
# the step invokes cargo fmt directly — so this selftest pins the step's
# mechanism: the pinned toolchain's rustfmt must reject drift (exit nonzero
# on an unformatted fixture crate) and accept a formatted one.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
channel="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$REPO_ROOT/rust-toolchain.toml")"
[ -n "$channel" ] || { echo "FAIL: could not read the pinned channel from rust-toolchain.toml"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
cat > "$fixture/Cargo.toml" <<'EOF'
[package]
name = "fmt-selftest-fixture"
version = "0.0.0"
edition = "2021"

[workspace]
EOF
mkdir "$fixture/src"
printf 'pub fn add(a:i32,b:i32)->i32{a+b}\n' > "$fixture/src/lib.rs"

failures=0

set +e
(cd "$fixture" && RUSTUP_TOOLCHAIN="$channel" cargo fmt --check) >"$fixture/out" 2>&1
rc=$?
set -e
if [ "$rc" -eq 0 ]; then
  echo "FAIL (drift case): cargo fmt --check accepted an unformatted fixture (exit 0)"
  failures=$((failures + 1))
else
  echo "OK (drift case): cargo fmt --check rejected the unformatted fixture"
fi

(cd "$fixture" && RUSTUP_TOOLCHAIN="$channel" cargo fmt) >/dev/null 2>&1
set +e
(cd "$fixture" && RUSTUP_TOOLCHAIN="$channel" cargo fmt --check) >/dev/null 2>&1
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
  echo "FAIL (formatted case): expected 0 after cargo fmt, got $rc"
  failures=$((failures + 1))
else
  echo "OK (formatted case): cargo fmt --check accepted the formatted fixture"
fi

if [ "$failures" -eq 0 ]; then
  echo "SELFTEST OK: the fmt step's mechanism is alive (drift rejected, formatted accepted)"
  exit 0
fi
echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
exit 1
