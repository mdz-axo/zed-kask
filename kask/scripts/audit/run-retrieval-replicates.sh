#!/usr/bin/env bash
# run-retrieval-replicates.sh — run the sharded chunk-retrieval evaluation k
# times and report the median and spread across replicates.
#
# WHY REPLICATES
#
# The query-embedding path is not deterministic: the same query text returns
# different vectors per call (measured on a binary predating this work — six
# sequential identical queries produced three distinct top-1 scores). A single
# evaluation pass is therefore one draw from a distribution, and comparing a
# new number to a recorded baseline as if both were exact would manufacture
# signal that is not there. This driver takes k independent draws so the
# report can state a median and the observed spread, and so a difference
# smaller than the spread is not claimed as a change.
#
# Each replicate is a full sharded evaluation in its own output directory;
# replicates are independent (fresh server processes per shard).
#
# USAGE
#   run-retrieval-replicates.sh <k> <shards> <output-root> <policy> <mode> \
#     <queries-jsonl> <retrieval-representation-jsonl> <index-db> <top-k> \
#     <word-budget> [<child-parent-map-jsonl> <parent-representation-jsonl>]
set -euo pipefail

usage() {
    cat >&2 <<'EOF'
usage: run-retrieval-replicates.sh <k> <shards> <output-root> <policy> <mode> \
  <queries-jsonl> <retrieval-representation-jsonl> <index-db> <top-k> \
  <word-budget> [<child-parent-map-jsonl> <parent-representation-jsonl>]
EOF
    exit 64
}

if [[ $# -ne 10 && $# -ne 12 ]]; then
    usage
fi

replicates=$1
shards=$2
output_root=$3
policy=$4
mode=$5
queries=$6
retrieval_representation=$7
index_db=$8
top_k=$9
word_budget=${10}
child_parent_map=${11:-}
parent_representation=${12:-}

if [[ ! "$replicates" =~ ^[1-9][0-9]*$ ]]; then
    echo "k must be a positive integer" >&2
    exit 64
fi
if [[ ! "$shards" =~ ^[1-9][0-9]*$ ]]; then
    echo "shards must be a positive integer" >&2
    exit 64
fi
if [[ -e "$output_root" ]]; then
    echo "refusing to overwrite output root: $output_root" >&2
    exit 73
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
sharded="$script_dir/evaluate-chunk-retrieval-sharded.sh"
if [[ ! -x "$sharded" ]]; then
    echo "sharded evaluator not found or not executable: $sharded" >&2
    exit 66
fi

mkdir -p "$output_root"

replicate=1
while (( replicate <= replicates )); do
    replicate_dir="$output_root/replicate-$(printf '%02d' "$replicate")"
    echo "replicate $replicate/$replicates -> $replicate_dir" >&2
    if [[ "$mode" == "small-to-big" ]]; then
        "$sharded" "$policy" "$mode" "$queries" "$retrieval_representation" \
            "$index_db" "$replicate_dir" "$top_k" "$word_budget" "$shards" \
            "$child_parent_map" "$parent_representation"
    else
        "$sharded" "$policy" "$mode" "$queries" "$retrieval_representation" \
            "$index_db" "$replicate_dir" "$top_k" "$word_budget" "$shards"
    fi
    replicate=$(( replicate + 1 ))
done

# Aggregate: median and observed spread for every scalar metric, plus the
# per-metric min/max so a reader can see the noise floor directly rather than
# inferring it from a median alone.
aggregate="$output_root/replicates.json"
jq -s '
  def scalars: [
    "exact_evidence_recall_at_5", "exact_evidence_recall_at_20",
    "correct_source_recall_at_5", "correct_source_recall_at_20",
    "exact_evidence_mrr", "correct_source_mrr",
    "budgeted_exact_evidence_recall", "budgeted_correct_source_recall",
    "retrieved_words.mean", "retrieved_words.total",
    "duplicate_overlap.rate"
  ];
  def get_path($obj; $path):
    reduce ($path | split("."))[] as $key ($obj; .[$key]);
  . as $runs
  | {
      replicates: ($runs | length),
      per_metric: (
        scalars as $metrics
        | reduce $metrics[] as $metric ({};
            ([$runs[] | get_path(.; $metric) | select(. != null)]) as $values
            | if ($values | length) == 0 then .
              else .[$metric] = {
                values: $values,
                min: ($values | min),
                max: ($values | max),
                median: ($values | sort | .[((length - 1) / 2 | floor)]),
                spread: (($values | max) - ($values | min))
              }
              end)
      ),
      gates: {
        source_fidelity: ([$runs[].source_fidelity_gate] | unique),
        query_counts: ([$runs[].query_count] | unique)
      }
    }
' "$output_root"/replicate-*/summary.json > "$aggregate"

echo "replicate aggregate: $aggregate" >&2
jq -r '.per_metric | to_entries[] | "  \(.key): median=\(.value.median) min=\(.value.min) max=\(.value.max) spread=\(.value.spread)"' "$aggregate" >&2
