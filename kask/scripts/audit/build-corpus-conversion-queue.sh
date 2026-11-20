#!/usr/bin/env bash
set -euo pipefail

# Build a conversion queue manifest from a directory of staged sources: one
# unit per immediate regular file, with the source content hash recorded so
# the wave executor can verify input identity on every run. Sources whose
# output and report both already exist are marked completed (resume by
# receipt); an output without its report is an interrupted conversion and is
# marked failed_missing_report for operator reconciliation.

if [[ $# -ne 3 ]]; then
    echo "usage: $0 <sources-dir> <output-dir> <queue-jsonl>" >&2
    exit 64
fi

sources_dir=$1
output_dir=$2
queue=$3

for command in jq sha256sum find; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "required command not found: $command" >&2
        exit 69
    fi
done
if [[ ! -d "$sources_dir" ]]; then
    echo "sources directory does not exist: $sources_dir" >&2
    exit 66
fi
if [[ -e "$queue" ]]; then
    echo "refusing to overwrite queue: $queue" >&2
    exit 73
fi
mkdir -p "$output_dir" "$(dirname "$queue")"

unit_ordinal=0
completed=0
pending=0
interrupted=0
queue_tmp=$(mktemp "${queue}.tmp.XXXXXX")
: > "$queue_tmp"
while IFS= read -r -d '' source; do
    name=$(basename "$source")
    unit_ordinal=$((unit_ordinal + 1))
    unit=$(printf 'convert-%03d-%s' "$unit_ordinal" "${name%.*}")
    output="$output_dir/$name.txt"
    report="$output.report.json"
    args="$output_dir/.receipts/$unit-args.json"
    response="$output_dir/.receipts/$unit-response.json"
    log="$output_dir/.receipts/$unit-server.log"
    source_sha256=$(sha256sum "$source" | cut -d' ' -f1)
    extension="${name##*.}"
    case "${extension,,}" in
        pdf) type=pdf ;;
        html|htm) type=html ;;
        *) type=other ;;
    esac
    if [[ -f "$output" && -f "$report" ]]; then
        status=completed
        completed=$((completed + 1))
        note="resumed from existing receipts"
    elif [[ -f "$output" ]]; then
        status=failed_missing_report
        interrupted=$((interrupted + 1))
        note="interrupted conversion: output exists without its report; inspect and delete the orphan before re-queuing"
    else
        status=pending
        pending=$((pending + 1))
        note=""
    fi
    jq -cn --arg unit "$unit" --arg source "$source" --arg source_sha256 "$source_sha256" \
        --arg type "$type" --arg output "$output" --arg report "$report" \
        --arg args "$args" --arg response "$response" --arg log "$log" \
        --arg status "$status" --arg note "$note" \
        '{unit:$unit,ordinal:1,source:$source,source_sha256:$source_sha256,type:$type,
          output:$output,report:$report,args:$args,response:$response,log:$log,
          status:$status,note:$note}' >> "$queue_tmp"
done < <(find "$sources_dir" -maxdepth 1 -type f -print0 | sort -z)

if [[ ! -s "$queue_tmp" ]]; then
    rm -f "$queue_tmp"
    echo "sources directory contains no regular files: $sources_dir" >&2
    exit 65
fi
chmod --reference="$(dirname "$queue")" "$queue_tmp" 2>/dev/null || true
mv -f "$queue_tmp" "$queue"
printf 'conversion queue built: units=%d completed=%d pending=%d interrupted=%d\n' \
    "$unit_ordinal" "$completed" "$pending" "$interrupted" >&2
