#!/usr/bin/env bash
# Run `cargo test` with pass-through args; fail when zero tests executed.
#
# libtest reports a filter that matches nothing (e.g. `-- --exact <leaf>`) as
# `running 0 tests` + `test result: ok`, and cargo exits 0 — a green run that
# checked nothing. Same contract as cargo-nextest `--no-tests=fail`
# (https://nexte.st/docs/running/). Executed = passed + failed, summed over
# every `test result:` line; ignored/filtered tests do not count.
#
# Usage: kask/scripts/cargo-test-nonzero.sh [cargo test args...]
set -euo pipefail

log="$(mktemp)"
trap 'rm -f "$log"' EXIT

status=0
cargo test "$@" 2>&1 | tee "$log" || status=${PIPESTATUS[0]}

if (( status != 0 )); then
  exit "$status"
fi

executed=0
results=0
while IFS= read -r line; do
  if [[ "$line" =~ test\ result:\ [a-zA-Z]+\.\ ([0-9]+)\ passed\;\ ([0-9]+)\ failed ]]; then
    executed=$(( executed + BASH_REMATCH[1] + BASH_REMATCH[2] ))
    results=$(( results + 1 ))
  fi
done < "$log"

if (( results == 0 )); then
  echo "cargo-test-nonzero: FAIL — no 'test result:' lines found; nothing verifiably ran" >&2
  exit 3
fi
if (( executed == 0 )); then
  echo "cargo-test-nonzero: FAIL — 0 tests executed across $results result line(s); the filter matched nothing" >&2
  exit 4
fi
echo "cargo-test-nonzero: OK — $executed test(s) executed across $results result line(s)"
