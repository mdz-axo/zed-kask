#!/usr/bin/env bash
# evaluate-chunk-retrieval-sharded.sh — fan a chunk-retrieval evaluation out
# across N concurrent shards, then reconcile and delegate scoring to the
# canonical evaluator.
#
# WHY THIS EXISTS
#
# evaluate-chunk-retrieval.sh drives one long-lived MCP server and issues
# queries strictly sequentially. At zk-ref-open scale that is ~10.5 s per
# query (index hydration dominates), so a 331-query run costs ~58 minutes on
# a 24-core machine that is idle for almost all of it. The per-query work is
# embarrassingly parallel — the retrieval legs fan out and the server's
# resident set is ~30 MB — so the sequential driver leaves the machine mostly
# unused. Measured on this host: 4 concurrent one-shot calls completed in
# 6.8 s where the same 4 sequentially took 35.6 s (5.2x).
#
# This script does not reimplement any measurement. It partitions the query
# set into N contiguous shards, runs each shard through the canonical
# evaluator (so every shard's rows are produced by exactly the same code
# path, shape, and parameter recording as a full run), concatenates the raw
# results, and then re-invokes the canonical evaluator with --reuse-raw so
# the scoring, reconciliation, and summary are computed by the one
# implementation that owns them.
#
# A shard that fails fails the whole run: partial evidence is never scored.
#
# USAGE
#   evaluate-chunk-retrieval-sharded.sh <policy> <mode> <queries-jsonl> \
#     <retrieval-representation-jsonl> <index-db> <output-dir> <top-k> \
#     <word-budget> <shards> [<child-parent-map-jsonl> <parent-representation-jsonl>]
#
# The trailing two arguments are required when <mode> is small-to-big, matching
# the canonical evaluator's contract.
set -euo pipefail

usage() {
    cat >&2 <<'EOF'
usage: evaluate-chunk-retrieval-sharded.sh <policy> <mode> <queries-jsonl> \
  <retrieval-representation-jsonl> <index-db> <output-dir> <top-k> \
  <word-budget> <shards> [<child-parent-map-jsonl> <parent-representation-jsonl>]
EOF
    exit 64
}

if [[ $# -ne 9 && $# -ne 11 ]]; then
    usage
fi

policy=$1
mode=$2
queries=$3
retrieval_representation=$4
index_db=$5
output_dir=$6
top_k=$7
word_budget=$8
shards=$9
child_parent_map=${10:-}
parent_representation=${11:-}

if [[ ! "$shards" =~ ^[1-9][0-9]*$ ]]; then
    echo "shards must be a positive integer" >&2
    exit 64
fi
if [[ "$mode" == "small-to-big" && $# -ne 11 ]]; then
    usage
fi
if [[ "$mode" == "direct" && $# -ne 9 ]]; then
    usage
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
evaluator="$script_dir/evaluate-chunk-retrieval.sh"
if [[ ! -x "$evaluator" ]]; then
    echo "canonical evaluator not found or not executable: $evaluator" >&2
    exit 66
fi
for path in "$queries" "$retrieval_representation" "$index_db"; do
    if [[ ! -f "$path" ]]; then
        echo "required file does not exist: $path" >&2
        exit 66
    fi
done
if [[ -e "$output_dir" ]]; then
    echo "refusing to overwrite output directory: $output_dir" >&2
    exit 73
fi

query_count=$(wc -l < "$queries" | tr -d ' ')
if (( query_count == 0 )); then
    echo "query set is empty: $queries" >&2
    exit 65
fi
if (( shards > query_count )); then
    echo "shards ($shards) exceeds query count ($query_count) — every shard must carry at least one query" >&2
    exit 64
fi

work_root="${output_dir}.shards"
mkdir -p "$work_root"

cleanup_work_root() {
    # Shard raw results are the only irreplaceable product until the merge
    # completes; on failure they are left in place for inspection and the
    # operator is told where.
    if [[ "${merge_succeeded:-false}" == true ]]; then
        rm -rf "$work_root"
    else
        echo "shard artifacts retained for inspection: $work_root" >&2
    fi
}
merge_succeeded=false
trap cleanup_work_root EXIT

# Contiguous shards: each shard's rows keep their global query order, so the
# merged file is order-equivalent to a sequential run.
base=$(( query_count / shards ))
remainder=$(( query_count % shards ))
offset=1
shard_index=0
shard_dirs=()
while (( shard_index < shards )); do
    size=$base
    if (( shard_index < remainder )); then
        size=$(( size + 1 ))
    fi
    if (( size == 0 )); then
        break
    fi
    shard_dir="$work_root/shard-$(printf '%03d' "$shard_index")"
    mkdir -p "$shard_dir"
    sed -n "${offset},$(( offset + size - 1 ))p" "$queries" > "$shard_dir/queries.jsonl"
    shard_dirs+=("$shard_dir")
    offset=$(( offset + size ))
    shard_index=$(( shard_index + 1 ))
done

echo "launching ${#shard_dirs[@]} shards over $query_count queries" >&2

pids=()
for shard_dir in "${shard_dirs[@]}"; do
    (
        if [[ "$mode" == "small-to-big" ]]; then
            "$evaluator" "$policy" "$mode" \
                "$shard_dir/queries.jsonl" "$retrieval_representation" "$index_db" \
                "$shard_dir/out" "$top_k" "$word_budget" \
                "$child_parent_map" "$parent_representation" \
                > "$shard_dir/evaluator.stdout" 2>&1
        else
            "$evaluator" "$policy" "$mode" \
                "$shard_dir/queries.jsonl" "$retrieval_representation" "$index_db" \
                "$shard_dir/out" "$top_k" "$word_budget" \
                > "$shard_dir/evaluator.stdout" 2>&1
        fi
    ) &
    pids+=("$!")
done

failed=0
for pid in "${pids[@]}"; do
    if ! wait "$pid"; then
        failed=$(( failed + 1 ))
    fi
done
if (( failed > 0 )); then
    echo "$failed of ${#pids[@]} shard(s) failed — no partial evidence is scored" >&2
    for shard_dir in "${shard_dirs[@]}"; do
        if [[ -s "$shard_dir/evaluator.stdout" ]]; then
            echo "--- $shard_dir ---" >&2
            tail -5 "$shard_dir/evaluator.stdout" >&2
        fi
    done
    exit 65
fi

merged_raw="$work_root/raw-results.merged.jsonl"
: > "$merged_raw"
for shard_dir in "${shard_dirs[@]}"; do
    shard_raw="$shard_dir/out/raw-results.jsonl"
    if [[ ! -s "$shard_raw" ]]; then
        echo "shard produced no raw results: $shard_raw" >&2
        exit 65
    fi
    cat "$shard_raw" >> "$merged_raw"
done

# The merge must reconstitute exactly the planned query set: a shard that
# silently dropped or duplicated a row would otherwise be scored as complete.
if ! jq -s -e --slurpfile planned "$queries" '
    length == ($planned | length) and
    ([.[].query_id] | length == (unique | length)) and
    all(.[]; .query_id as $id | any($planned[]; .query_id == $id))
' "$merged_raw" >/dev/null; then
    echo "merged raw results do not reconcile with the planned query set" >&2
    exit 65
fi

echo "reconciled $(wc -l < "$merged_raw" | tr -d ' ')/$query_count rows; scoring via the canonical evaluator" >&2

# The canonical evaluator's --reuse-raw path takes the embedding model from
# the environment rather than the host process (no server is launched, so
# there is nothing to read it from). Carry it across if it is not already
# set, reading the host's value the same way the evaluator does.
if [[ -z ${HKASK_EMBEDDING_MODEL:-} ]]; then
    for candidate in $(pgrep -f 'hkask-mcp-corpus' 2>/dev/null || true); do
        model=$(tr '\0' '\n' < "/proc/$candidate/environ" 2>/dev/null | sed -n 's/^HKASK_EMBEDDING_MODEL=//p' | head -1 || true)
        if [[ -n $model ]]; then
            HKASK_EMBEDDING_MODEL=$model
            export HKASK_EMBEDDING_MODEL
            break
        fi
    done
fi

# The canonical evaluator creates its own output directory and refuses to
# run against one that already exists — do not pre-create it here.
if [[ "$mode" == "small-to-big" ]]; then
    "$evaluator" --reuse-raw "$merged_raw" "$policy" "$mode" \
        "$queries" "$retrieval_representation" "$index_db" \
        "$output_dir" "$top_k" "$word_budget" \
        "$child_parent_map" "$parent_representation"
else
    "$evaluator" --reuse-raw "$merged_raw" "$policy" "$mode" \
        "$queries" "$retrieval_representation" "$index_db" \
        "$output_dir" "$top_k" "$word_budget"
fi

merge_succeeded=true
echo "sharded evaluation complete: $output_dir/summary.json" >&2
