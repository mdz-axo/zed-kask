#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 6 && $# -ne 7 ]]; then
    echo "usage: $0 <queue-jsonl> <corpus-tool-runner> <max-cost-usd> <concurrency> <tag-batch-size> <estimated-cost-per-chunk-usd> [max-units]" >&2
    exit 64
fi

queue=$1
runner=$2
max_cost=$3
concurrency=$4
tag_batch_size=$5
estimated_cost_per_chunk=$6
max_units=${7:-0}

for path in "$queue" "$runner"; do
    if [[ ! -f "$path" ]]; then
        echo "required file does not exist: $path" >&2
        exit 66
    fi
done
for command in jq sha256sum cmp; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "required command not found: $command" >&2
        exit 69
    fi
done
if [[ ! "$concurrency" =~ ^[1-9][0-9]*$ || ! "$tag_batch_size" =~ ^[1-9][0-9]*$ ]]; then
    echo "concurrency and tag-batch-size must be positive integers" >&2
    exit 64
fi
if [[ ! "$max_units" =~ ^[0-9]+$ ]]; then
    echo "max-units must be a nonnegative integer" >&2
    exit 64
fi
for value in "$max_cost" "$estimated_cost_per_chunk"; do
    if ! jq -en --arg value "$value" '$value | tonumber | . >= 0' >/dev/null 2>&1; then
        echo "cost values must be nonnegative numbers" >&2
        exit 64
    fi
done
jq -s -e '
    length > 0 and
    ([.[].unit] | length == (unique | length)) and
    all(.[];
      (.unit | type == "string" and length > 0) and
      (.rows | type == "number" and . > 0) and
      (.input | type == "string" and length > 0) and
      (.output | type == "string" and length > 0) and
      (.args | type == "string" and length > 0) and
      (.response | type == "string" and length > 0) and
      (.log | type == "string" and length > 0) and
      (.status == "pending" or .status == "completed" or .status == "reconciled_partial" or (.status | startswith("failed"))))
' "$queue" >/dev/null

update_unit() {
    local unit=$1
    local patch=$2
    local tmp
    tmp=$(mktemp "${queue}.tmp.XXXXXX")
    jq -c --arg unit "$unit" --argjson patch "$patch" \
        'if .unit == $unit then . + $patch else . end' "$queue" > "$tmp"
    chmod --reference="$queue" "$tmp"
    mv -f "$tmp" "$queue"
}

spent=$(jq -s '[.[] | if .status == "completed" then .reported_cost_usd elif .status == "reconciled_partial" and .cost_reporting_complete == true then .reported_cost_usd elif .status == "reconciled_partial" then .reserved_cost_usd else empty end] | add // 0' "$queue")
mapfile -t units < "$queue"
planned=${#units[@]}
completed=$(jq -s '[.[] | select(.status == "completed")] | length' "$queue")
processed_this_run=0

for row in "${units[@]}"; do
    unit=$(jq -r '.unit' <<<"$row")
    status=$(jq -r '.status' <<<"$row")
    rows=$(jq -r '.rows' <<<"$row")
    input=$(jq -r '.input' <<<"$row")
    output=$(jq -r '.output' <<<"$row")
    args=$(jq -r '.args' <<<"$row")
    response=$(jq -r '.response' <<<"$row")
    log=$(jq -r '.log' <<<"$row")

    if [[ "$status" == completed || "$status" == reconciled_partial ]]; then
        if [[ ! -f "$output" || ! -f "$response" || ! -f "$log" ]]; then
            echo "terminal unit is missing durable artifacts: $unit" >&2
            exit 65
        fi
        continue
    fi
    if [[ "$status" != pending ]]; then
        echo "queue contains unresolved failed unit $unit with status $status; reconcile it before resume" >&2
        exit 65
    fi
    if (( max_units > 0 && processed_this_run >= max_units )); then
        break
    fi
    if [[ ! -f "$input" ]]; then
        echo "pending unit input does not exist: $input" >&2
        exit 66
    fi
    actual_input_rows=$(wc -l < "$input" | tr -d ' ')
    if (( actual_input_rows != rows )); then
        echo "queue/input row mismatch for $unit: queue=$rows actual=$actual_input_rows" >&2
        exit 65
    fi

    estimated_unit_cost=$(jq -n --argjson rows "$rows" --argjson rate "$estimated_cost_per_chunk" '$rows * $rate')
    if ! jq -en --argjson spent "$spent" --argjson estimate "$estimated_unit_cost" --argjson cap "$max_cost" '$spent + $estimate <= $cap' >/dev/null; then
        echo "cost gate blocks $unit: spent=$spent estimated_unit=$estimated_unit_cost cap=$max_cost" >&2
        exit 75
    fi

    for path in "$output" "$response" "$log"; do
        if [[ -e "$path" ]]; then
            echo "pending unit has unreconciled artifact: $path" >&2
            exit 73
        fi
        mkdir -p "$(dirname "$path")"
    done
    if [[ -e "$args" ]]; then
        if ! jq -e --arg input "$input" --arg output "$output" --argjson concurrency "$concurrency" --argjson batch "$tag_batch_size" '
            .chunks_jsonl == $input and .output == $output and
            .concurrency == $concurrency and .tag_batch_size == $batch and .dry_run == false
        ' "$args" >/dev/null; then
            echo "existing arguments do not match queue policy: $args" >&2
            exit 65
        fi
    else
        mkdir -p "$(dirname "$args")"
        jq -n --arg input "$input" --arg output "$output" --argjson concurrency "$concurrency" --argjson batch "$tag_batch_size" \
            '{chunks_jsonl:$input,output:$output,concurrency:$concurrency,tag_batch_size:$batch,dry_run:false}' > "$args"
    fi

    started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    update_unit "$unit" "$(jq -cn --arg started_at "$started_at" '{status:"running",started_at:$started_at}')"
    if ! "$runner" corpus_tag_chunks "$args" "$response" "$log"; then
        failed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
        update_unit "$unit" "$(jq -cn --arg failed_at "$failed_at" '{status:"failed_runner",failed_at:$failed_at,error:"corpus tool runner failed; inspect durable response/log/output before retry"}')"
        echo "classification runner failed for $unit" >&2
        exit 1
    fi

    summary=$(jq -cer '
        [.result.content[]? | select(.type == "text") | .text | fromjson | .content] as $content
        | if ($content | length) == 1 then $content[0]
          else error("unexpected corpus_tag_chunks response envelope") end
    ' "$response")
    tagged=$(jq -r '.tagged' <<<"$summary")
    failed=$(jq -r '.failed' <<<"$summary")
    total=$(jq -r '.total_chunks' <<<"$summary")
    cost_complete=$(jq -r '.cost_reporting_complete' <<<"$summary")
    cost=$(jq -r '.reported_cost_usd' <<<"$summary")
    output_rows=$(wc -l < "$output" | tr -d ' ')

    if (( total != rows || tagged + failed != total || output_rows != rows )); then
        failed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
        update_unit "$unit" "$(jq -cn --arg failed_at "$failed_at" --argjson total "$total" --argjson tagged "$tagged" --argjson failed "$failed" --argjson output_rows "$output_rows" '{status:"failed_reconciliation",failed_at:$failed_at,total:$total,tagged:$tagged,failed:$failed,output_rows:$output_rows}')"
        echo "classification count reconciliation failed for $unit" >&2
        exit 65
    fi
    if [[ "$cost_complete" != true ]] || ! jq -en --arg value "$cost" '$value | tonumber | . >= 0' >/dev/null 2>&1; then
        failed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
        update_unit "$unit" "$(jq -cn --arg failed_at "$failed_at" '{status:"failed_cost_reporting",failed_at:$failed_at,error:"provider cost reporting incomplete"}')"
        echo "classification cost reporting incomplete for $unit" >&2
        exit 65
    fi

    input_refs=$(mktemp)
    output_refs=$(mktemp)
    trap 'rm -f "$input_refs" "$output_refs"' EXIT
    jq -r '.entity_ref' "$input" | sort > "$input_refs"
    jq -r '.entity_ref' "$output" | sort > "$output_refs"
    if ! cmp -s "$input_refs" "$output_refs" || ! jq -s -e --argjson rows "$rows" '
        length == $rows and
        ([.[].entity_ref] | length == (unique | length)) and
        all(.[];
          .classification.status == "classified" and
          .classification.ontology_protocol == "published-term-resolution-v1" and
          (.candidate_terms | type == "array" and length >= 3 and length <= 5) and
          (.ontology_tags | type == "object") and
          (.concepts | type == "array"))
    ' "$output" >/dev/null; then
        rm -f "$input_refs" "$output_refs"
        trap - EXIT
        failed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
        update_unit "$unit" "$(jq -cn --arg failed_at "$failed_at" '{status:"failed_identity_or_protocol",failed_at:$failed_at,error:"identity, uniqueness, protocol, or canonical-anchor shape check failed"}')"
        echo "classification identity/protocol reconciliation failed for $unit" >&2
        exit 65
    fi
    rm -f "$input_refs" "$output_refs"
    trap - EXIT
    if (( failed != 0 )); then
        failed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
        update_unit "$unit" "$(jq -cn --arg failed_at "$failed_at" --argjson failed "$failed" '{status:"failed_classification",failed_at:$failed_at,failed:$failed}')"
        echo "classification produced failed rows for $unit" >&2
        exit 1
    fi

    spent=$(jq -n --argjson spent "$spent" --argjson cost "$cost" '$spent + $cost')
    completed=$((completed + 1))
    processed_this_run=$((processed_this_run + 1))
    completed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    patch=$(jq -cn --arg completed_at "$completed_at" --arg output_sha256 "$(sha256sum "$output" | cut -d' ' -f1)" --argjson tagged "$tagged" --argjson failed "$failed" --argjson cost "$cost" --argjson cost_complete "$cost_complete" --argjson cumulative_cost "$spent" '{status:"completed",completed_at:$completed_at,tagged:$tagged,failed:$failed,reported_cost_usd:$cost,cost_reporting_complete:$cost_complete,cumulative_reported_cost_usd:$cumulative_cost,output_sha256:$output_sha256}')
    update_unit "$unit" "$patch"
    printf 'unit=%s completed=%d/%d rows=%d cumulative_cost_usd=%s\n' "$unit" "$completed" "$planned" "$rows" "$spent" >&2

    if ! jq -en --argjson spent "$spent" --argjson cap "$max_cost" '$spent <= $cap' >/dev/null; then
        echo "actual cumulative cost reached beyond authorized cap after $unit: spent=$spent cap=$max_cost" >&2
        exit 75
    fi
done

pending=$(jq -s '[.[] | select(.status == "pending")] | length' "$queue")
if (( pending == 0 )); then
    printf 'classification_queue_complete units=%d cumulative_accounted_cost_usd=%s\n' "$planned" "$spent" >&2
else
    printf 'classification_queue_checkpoint processed_this_run=%d completed_units=%d pending_units=%d cumulative_accounted_cost_usd=%s\n' \
        "$processed_this_run" "$completed" "$pending" "$spent" >&2
fi
