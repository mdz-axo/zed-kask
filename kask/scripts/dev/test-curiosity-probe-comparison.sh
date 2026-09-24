#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
script="$here/curiosity-probe-comparison.sh"
output="$(bash "$script" --runs 2 --trace)"

[[ "$(grep -c '^ARM ' <<<"$output")" -eq 6 ]]
[[ "$(grep -c '^PROBE ' <<<"$output")" -eq 72 ]]
[[ "$(grep -c '^SUMMARY ' <<<"$output")" -eq 3 ]]
awk '/^ARM / {if ($0 !~ /seed=6 budget=12 used=12/) exit 1; n++} END {if (n != 6) exit 1}' <<<"$output"
awk '/^PROBE / {split($0, a, "cell="); split(a[2], b, " "); if (b[1] < 0 || b[1] >= 48) exit 1; n++} END {if (n != 72) exit 1}' <<<"$output"
[[ "$(bash "$script" --runs 2 --trace)" == "$output" ]]
# Every arm must pay for confirming the flaky seed, whose first observed pass
# contradicts the stable oracle's failure. A repeat cannot be a new discovery.
[[ "$(grep -c '^PROBE .*step=3 cell=24 .*observed=0 truth=0$' <<<"$output")" -eq 6 ]]
[[ "$output" == *'SUMMARY policy=systematic runs=2 new_pass=7 unexpected_true_boundaries=2 false_positive=0'* ]]
[[ "$output" == *'SUMMARY policy=orchestration-proxy runs=2 new_pass=6 unexpected_true_boundaries=2 false_positive=0'* ]]
[[ "$output" == *'SUMMARY policy=progress runs=2 new_pass=5 unexpected_true_boundaries=5 false_positive=0'* ]]
if bash "$script" --runs 0 >/dev/null 2>&1; then
    echo 'zero runs must be rejected' >&2
    exit 1
fi
printf 'synthetic comparison contract: PASS\n'
