#!/usr/bin/env bash
# Mechanical-floor cross-check over the 40 baseline runs (per the
# trust-adherence-v1 adjudication protocol: the calibrated adjudicator
# is the measurement; this script quantifies floor-vs-adjudicator
# disagreement for the record).
set -u
cd "$(dirname "$0")/../.." || exit 1
cd "$(git rev-parse --show-toplevel 2>/dev/null || pwd)" || exit 1
R=kask/eval/trust-adherence-v1/baseline
total=0; pass=0
fails=""
for id in R1 R2 R3 R4 R5 C1 C2 C3 C4 C5 D1 D2 D3 D4 D5 V1 V2 V3 V4 V5; do
  for rep in r1 r2; do
    out=$(cargo run -q -p hkask-trust-eval --bin evaltool -- check "$id" --output "$R/$id-$rep.txt" 2>/dev/null)
    n=$(printf '%s' "$out" | jq -c '.instructions[]' | wc -l)
    for k in $(seq 0 $((n - 1))); do
      row=$(printf '%s' "$out" | jq -c ".instructions[$k]")
      instr=$(printf '%s' "$row" | jq -r '.instruction')
      mech=$(printf '%s' "$row" | jq -r '.mechanical_pass')
      total=$((total + 1))
      if [ "$mech" = "true" ]; then pass=$((pass + 1)); else fails="$fails $id-$rep:$instr"; fi
    done
  done
done
echo "floor: $pass / $total instructions mechanically pass"
echo "floor failures:${fails:- (none)}"
