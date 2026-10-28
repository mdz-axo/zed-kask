#!/usr/bin/env bash
# Exercise cargo-test-nonzero.sh against a fake `cargo` so every case is
# deterministic and needs no build: nonzero pass, zero-test pass-through,
# no result lines, and cargo's own failure exit propagating.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
wrapper="$repo_root/kask/scripts/cargo-test-nonzero.sh"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

# Fake cargo: prints $FAKE_OUT and exits $FAKE_EXIT.
cat > "$fixture/cargo" <<'EOF'
#!/usr/bin/env bash
printf '%b' "$FAKE_OUT"
exit "${FAKE_EXIT:-0}"
EOF
chmod +x "$fixture/cargo"

run() { PATH="$fixture:$PATH" FAKE_OUT="$1" FAKE_EXIT="$2" bash "$wrapper" some filter >/dev/null 2>&1; }

expect() {
  local name=$1 want=$2 out=$3 code=$4 got=0
  run "$out" "$code" || got=$?
  if (( got != want )); then
    echo "FAIL ($name): expected exit $want, got $got" >&2
    exit 1
  fi
  echo "OK ($name): exit $got"
}

expect "tests ran" 0 'running 3 tests\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\nrunning 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n' 0
expect "zero tests across binaries" 4 'running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 12 filtered out\nrunning 0 tests\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\n' 0
expect "no result lines" 3 'Compiling foo\n' 0
expect "cargo failure propagates" 101 'running 1 test\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n' 101

echo 'OK: zero executed tests fail the wrapper; real runs and cargo failures pass through.'
