#!/usr/bin/env bash
# Driver: run the classifier eval over the model list in /tmp/bench_models.txt.
# LIVE progress: per-case lines (stderr from the eval script) stream to the
# terminal as they happen; a [i/N] counter marks each model. The per-model
# summary line (stdout) is collected and printed as a table at the end, and
# saved to /tmp/eval_full_out.txt.
set -euo pipefail
cd "$(dirname "$0")/../.."

LIST="${1:-/tmp/bench_models.txt}"
[ -f "$LIST" ] || { echo "error: model list not found: $LIST" >&2; exit 1; }
mapfile -t MODELS < "$LIST"
NMODELS=${#MODELS[@]}

echo "== classifier eval: $NMODELS models x 50 cases =="
echo "== per-case lines stream below; summary table at the end =="

SUMMARY=/tmp/eval_full_out.txt
: > "$SUMMARY"

i=0
for m in "${MODELS[@]}"; do
  i=$((i+1))
  echo "" >&2
  echo ">>> [$i/$NMODELS] $m" >&2
  # stdout = summary line(s) -> append to $SUMMARY (skip the header line)
  # stderr = per-case progress -> pass straight through to the terminal
  bash kask/scripts/check-classifier-models.sh "$m" 2>&1 >/tmp/one_model.out || true
  grep -v '^model|' /tmp/one_model.out >> "$SUMMARY" || true
  echo ">>> [$i/$NMODELS] $m done" >&2
done

echo ""
echo "================ SUMMARY ================"
echo "model|correct/47|inctx/3|section/20|dimension/20|failure/10|ttft_p50_ms|tok_s_p50"
cat "$SUMMARY"
