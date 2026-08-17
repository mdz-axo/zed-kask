#!/usr/bin/env bash
# Driver: run the classifier eval over a model list (default /tmp/bench_models.txt).
#
# Live progress: per-case lines stream to stderr as they happen (so they reach
# the terminal in the foreground AND a progress log when backgrounded). A
# [i/N] counter marks each model. The per-model summary line (stdout) is
# collected into /tmp/eval_full_out.txt and printed as a table at the end.
#
# The summary file is NEVER left silently empty: if a model's eval fails to
# produce a summary line, a `model|FAIL|...` row is written instead so the
# operator can see what broke (this was the original 0-byte-log bug).
#
# Usage: run-classifier-eval.sh [model-list-file]   (CONCURRENCY env tunes parallelism)
set -euo pipefail
cd "$(dirname "$0")/../.."
REPO_ROOT="$(pwd)"

LIST="${1:-/tmp/bench_models.txt}"
[ -f "$LIST" ] || { echo "error: model list not found: $LIST" >&2; exit 1; }
# Strip comments (#...) and blank lines so the bench list can be annotated.
mapfile -t MODELS < <(grep -vE '^[[:space:]]*(#|$)' "$LIST")
NMODELS=${#MODELS[@]}
CONCURRENCY="${CONCURRENCY:-8}"
export CONCURRENCY

SUMMARY="$REPO_ROOT/kask/docs/review/eval-full-out.txt"
PROGRESS="$REPO_ROOT/kask/docs/review/eval-full-log.txt"
: > "$SUMMARY"
: > "$PROGRESS"

echo "== classifier eval: $NMODELS models x 50 cases (concurrency $CONCURRENCY) ==" >&2
echo "== per-case lines stream below; summary table at the end ==" >&2

i=0
for m in "${MODELS[@]}"; do
  i=$((i+1))
  echo "" >&2
  echo ">>> [$i/$NMODELS] $m" >&2
  one="$(mktemp)"
  # stdout (summary) -> $one ; stderr (per-case progress) -> terminal AND progress log.
  if bash kask/scripts/check-classifier-models.sh "$m" > "$one" 2> >(tee -a "$PROGRESS" >&2); then
    # Skip the header line (model|correct/47|...); keep the data line.
    grep -v '^model|' "$one" >> "$SUMMARY" || echo "$m|FAIL|no-summary-line" >> "$SUMMARY"
  else
    rc=$?
    echo "$m|FAIL|eval-exit-$rc" >> "$SUMMARY"
    # Still surface whatever progress was captured.
    tee -a "$PROGRESS" < "$one" >&2 2>/dev/null || true
  fi
  rm -f "$one"
  echo ">>> [$i/$NMODELS] $m done" >&2
done

echo ""
echo "================ SUMMARY ================"
echo "model|correct/47|inctx/3|section/17|dimension/20|failure/10|ttft_p50_ms|tok_s_p50"
cat "$SUMMARY"
echo ""
echo "(per-case progress log: kask/docs/review/eval-full-log.txt ; summary: kask/docs/review/eval-full-out.txt)"