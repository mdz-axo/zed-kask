#!/usr/bin/env bash
set -euo pipefail

# Wave executor for bounded corpus_convert runs through the durable host
# wrapper: per-source receipts (output + report), resume by receipt, source
# content-hash verification on every run, and a strand guard so an unguarded
# exit inside a unit's running window can never strand it. Conversion
# responses carry no cost telemetry, so there is no cost gate here — paid OCR
# accounting lives where responses report it (the calibration runner and the
# embedding/QA queues).
#
# Waves:   run-corpus-conversion-queue.sh <queue-jsonl> <runner> [max-units]
# Requeue: run-corpus-conversion-queue.sh --requeue <queue-jsonl> <unit>
#          (a tool-error death wrote response+log but never output; the
#          requeue moves the stale receipts aside and resets the unit)

requeue_mode=false
if [[ "${1:-}" == "--requeue" ]]; then
    requeue_mode=true
    shift
fi

if [[ "$requeue_mode" == true ]]; then
    if [[ $# -ne 2 ]]; then
        echo "usage: $0 --requeue <queue-jsonl> <unit>" >&2
        exit 64
    fi
else
    if [[ $# -ne 2 && $# -ne 3 ]]; then
        echo "usage: $0 <queue-jsonl> <corpus-tool-runner> [max-units]" >&2
        exit 64
    fi
fi

queue=$1
if [[ "$requeue_mode" == true ]]; then
    unit=$2
else
    runner=$2
    max_units=${3:-0}
fi

for command in jq sha256sum; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "required command not found: $command" >&2
        exit 69
    fi
done
if [[ ! -f "$queue" ]]; then
    echo "queue does not exist: $queue" >&2
    exit 66
fi

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

strand_guard() {
    if [[ "${unit_terminal:-}" != true ]]; then
        local failed_at
        failed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
        update_unit "$unit" "$(jq -cn --arg failed_at "$failed_at" '{status:"failed_runner",failed_at:$failed_at,error:"queue exited between the running patch and a terminal patch; inspect the durable response and log, then requeue via --requeue"}')" || true
    fi
}

if [[ "$requeue_mode" == true ]]; then
    row=$(jq -sc --arg unit "$unit" '
        [.[] | select(.unit == $unit)]
        | if length == 1 then .[0] else error("unit must occur exactly once") end
    ' "$queue")
    status=$(jq -r '.status' <<<"$row")
    if [[ "$status" != "failed_runner" ]]; then
        echo "only a failed_runner unit can be requeued: $unit status=$status" >&2
        exit 65
    fi
    output=$(jq -r '.output' <<<"$row")
    response=$(jq -r '.response' <<<"$row")
    log=$(jq -r '.log' <<<"$row")
    source=$(jq -r '.source' <<<"$row")
    if [[ -f "$output" ]]; then
        echo "failed_runner unit unexpectedly has output: $output" >&2
        exit 65
    fi
    for path in "$response" "$log"; do
        if [[ ! -f "$path" ]]; then
            echo "failed_runner unit is missing its durable receipt: $path" >&2
            exit 66
        fi
    done
    if [[ -f "$source" ]]; then
        recorded=$(jq -r '.source_sha256 // empty' <<<"$row")
        if [[ -n "$recorded" ]]; then
            actual=$(sha256sum "$source" | cut -d' ' -f1)
            if [[ "$actual" != "$recorded" ]]; then
                echo "source content changed since the unit was built: $unit" >&2
                exit 65
            fi
        fi
    fi
    orphan_suffix="orphan-$(date -u +%Y%m%dT%H%M%S)-$$"
    for path in "$response" "$log"; do
        mv "$path" "${path}.${orphan_suffix}"
    done
    requeued_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    update_unit "$unit" "$(jq -cn --arg requeued_at "$requeued_at" --arg orphan_suffix "$orphan_suffix" \
        '{status:"pending",requeued_at:$requeued_at,orphaned_receipts_suffix:$orphan_suffix,
          error:"tool-error death discarded and requeued; stale receipts at the orphan suffix"}')"
    printf 'unit=%s status=pending requeued stale_receipts=*.%s\n' "$unit" "$orphan_suffix" >&2
    exit 0
fi

if [[ ! -f "$runner" ]]; then
    echo "runner does not exist: $runner" >&2
    exit 66
fi
if [[ ! "$max_units" =~ ^[0-9]+$ ]]; then
    echo "max-units must be a nonnegative integer" >&2
    exit 64
fi
jq -s -e '
    length > 0 and
    ([.[].unit] | length == (unique | length)) and
    all(.[];
      (.unit | type == "string" and length > 0) and
      (.source | type == "string" and length > 0) and
      (.source_sha256 | test("^[0-9a-f]{64}$")) and
      (.output | type == "string" and length > 0) and
      (.report | type == "string" and length > 0) and
      (.args | type == "string" and length > 0) and
      (.response | type == "string" and length > 0) and
      (.log | type == "string" and length > 0) and
      (.status == "pending" or .status == "completed" or (.status | startswith("failed"))))
' "$queue" >/dev/null

mapfile -t units < "$queue"
planned=${#units[@]}
completed=$(jq -s '[.[] | select(.status == "completed")] | length' "$queue")
processed_this_run=0
unresolved_failed=0

for row in "${units[@]}"; do
    unit=$(jq -r '.unit' <<<"$row")
    status=$(jq -r '.status' <<<"$row")
    source=$(jq -r '.source' <<<"$row")
    output=$(jq -r '.output' <<<"$row")
    report=$(jq -r '.report' <<<"$row")
    args=$(jq -r '.args' <<<"$row")
    response=$(jq -r '.response' <<<"$row")
    log=$(jq -r '.log' <<<"$row")

    if [[ "$status" == completed || "$status" == failed* ]]; then
        if [[ "$status" == "failed_runner" ]]; then
            if [[ ! -f "$response" || ! -f "$log" ]]; then
                echo "failed_runner unit is missing durable receipts: $unit" >&2
                exit 65
            fi
        else
            if [[ ! -f "$output" || ! -f "$report" ]]; then
                echo "terminal unit is missing durable artifacts: $unit" >&2
                exit 65
            fi
        fi
        if [[ "$status" == failed* ]]; then
            unresolved_failed=$((unresolved_failed + 1))
        fi
        continue
    fi
    if [[ "$status" != pending ]]; then
        echo "queue contains unsupported unit state $unit status=$status" >&2
        exit 65
    fi
    if (( max_units > 0 && processed_this_run >= max_units )); then
        break
    fi
    if [[ ! -f "$source" ]]; then
        echo "pending unit source does not exist: $source" >&2
        exit 66
    fi
    recorded_source_sha256=$(jq -r '.source_sha256' <<<"$row")
    actual_source_sha256=$(sha256sum "$source" | cut -d' ' -f1)
    if [[ "$actual_source_sha256" != "$recorded_source_sha256" ]]; then
        echo "source content changed since the queue was built: $unit" >&2
        exit 65
    fi

    for path in "$output" "$report" "$response" "$log"; do
        if [[ -e "$path" ]]; then
            echo "pending unit has an unreconciled artifact: $path" >&2
            exit 73
        fi
        mkdir -p "$(dirname "$path")"
    done
    if [[ -e "$args" ]]; then
        if ! jq -e --arg path "$source" --arg output "$output" \
            '.path == $path and .output == $output' "$args" >/dev/null; then
            echo "existing arguments do not match the unit: $args" >&2
            exit 65
        fi
    else
        mkdir -p "$(dirname "$args")"
        jq -n --arg path "$source" --arg output "$output" '{path:$path,output:$output}' > "$args"
    fi

    started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    update_unit "$unit" "$(jq -cn --arg started_at "$started_at" '{status:"running",started_at:$started_at}')"
    unit_terminal=false
    trap 'strand_guard' EXIT
    if ! "$runner" corpus_convert "$args" "$response" "$log"; then
        failed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
        update_unit "$unit" "$(jq -cn --arg failed_at "$failed_at" '{status:"failed_runner",failed_at:$failed_at,error:"conversion runner failed; inspect the durable response and log (output is only written on success), then requeue via --requeue"}')"
        unit_terminal=true
        echo "conversion runner failed for $unit" >&2
        exit 1
    fi

    summary=$(jq -cer '
        [.result.content[]? | select(.type == "text") | .text | fromjson | .content] as $content
        | if ($content | length) == 1 then $content[0]
          else error("unexpected corpus_convert response envelope") end
    ' "$response")
    reported_report=$(jq -r '.report // empty' <<<"$summary")
    word_count=$(jq -r '.word_count // 0' <<<"$summary")
    if [[ "$reported_report" != "$report" || ! -f "$output" || ! -f "$report" ]]; then
        failed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
        update_unit "$unit" "$(jq -cn --arg failed_at "$failed_at" --arg reported_report "$reported_report" \
            '{status:"failed_reconciliation",failed_at:$failed_at,reported_report:$reported_report,
              error:"conversion receipts did not reconcile (output or report missing, or report path differs)"}')"
        unit_terminal=true
        echo "conversion receipt reconciliation failed for $unit" >&2
        exit 65
    fi

    processed_this_run=$((processed_this_run + 1))
    completed=$((completed + 1))
    completed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    output_sha256=$(sha256sum "$output" | cut -d' ' -f1)
    update_unit "$unit" "$(jq -cn --arg completed_at "$completed_at" --arg output_sha256 "$output_sha256" \
        --argjson word_count "$word_count" \
        '{status:"completed",completed_at:$completed_at,word_count:$word_count,output_sha256:$output_sha256}')"
    unit_terminal=true
    printf 'unit=%s completed=%d/%d words=%d\n' "$unit" "$completed" "$planned" "$word_count" >&2
done
trap - EXIT

pending=$(jq -s '[.[] | select(.status == "pending")] | length' "$queue")
if (( pending == 0 )); then
    printf 'conversion_queue_complete units=%d\n' "$planned" >&2
else
    printf 'conversion_queue_checkpoint processed_this_run=%d completed_units=%d pending_units=%d\n' \
        "$processed_this_run" "$completed" "$pending" >&2
fi
failed_terminal=$(jq -s '[.[] | select(.status | startswith("failed"))] | length' "$queue")
if (( failed_terminal > 0 || unresolved_failed > 0 )); then
    echo "conversion queue has $failed_terminal terminal failed units requiring reconciliation" >&2
    exit 65
fi
