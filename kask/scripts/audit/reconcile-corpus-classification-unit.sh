#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo "usage: $0 <queue-jsonl> <failed-unit> <estimated-cost-per-chunk-usd>" >&2
    exit 64
fi

queue=$1
unit=$2
estimated_cost_per_chunk=$3

for command in jq sort cmp sha256sum; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "required command not found: $command" >&2
        exit 69
    fi
done
if [[ ! -f "$queue" ]]; then
    echo "queue does not exist: $queue" >&2
    exit 66
fi
if ! jq -en --arg value "$estimated_cost_per_chunk" '$value | tonumber | . >= 0' >/dev/null 2>&1; then
    echo "estimated-cost-per-chunk-usd must be a nonnegative number" >&2
    exit 64
fi

row=$(jq -sc --arg unit "$unit" '
    [.[] | select(.unit == $unit)]
    | if length == 1 then .[0] else error("unit must occur exactly once") end
' "$queue")
status=$(jq -r '.status' <<<"$row")
if [[ "$status" != failed* ]]; then
    echo "unit is not in a failed terminal state: $unit status=$status" >&2
    exit 65
fi

rows=$(jq -r '.rows' <<<"$row")
input=$(jq -r '.input' <<<"$row")
output=$(jq -r '.output' <<<"$row")
response=$(jq -r '.response' <<<"$row")
log=$(jq -r '.log' <<<"$row")
for path in "$input" "$output" "$response" "$log"; do
    if [[ ! -f "$path" ]]; then
        echo "failed unit is missing durable artifact: $path" >&2
        exit 66
    fi
done

input_rows=$(wc -l < "$input" | tr -d ' ')
output_rows=$(wc -l < "$output" | tr -d ' ')
if (( input_rows != rows || output_rows != rows )); then
    echo "failed unit row reconciliation failed: expected=$rows input=$input_rows output=$output_rows" >&2
    exit 65
fi

input_refs=$(mktemp)
output_refs=$(mktemp)
failed_refs=$(mktemp)
recovery_tmp=$(mktemp)
queue_tmp=$(mktemp "${queue}.tmp.XXXXXX")
cleanup() {
    rm -f "$input_refs" "$output_refs" "$failed_refs" "$recovery_tmp" "$queue_tmp"
}
trap cleanup EXIT

jq -r '.entity_ref' "$input" | sort > "$input_refs"
jq -r '.entity_ref' "$output" | sort > "$output_refs"
if ! cmp -s "$input_refs" "$output_refs"; then
    echo "failed unit input/output identities differ" >&2
    exit 65
fi
if ! jq -s -e --argjson rows "$rows" '
    length == $rows and
    ([.[].entity_ref] | length == (unique | length)) and
    all(.[];
      if .classification.status == "classified" then
        .classification.ontology_protocol == "published-term-resolution-v1" and
        (.candidate_terms | type == "array" and length >= 3 and length <= 5) and
        (.ontology_tags | type == "object") and
        (.concepts | type == "array")
      elif .classification.status == "failed" then
        (.classification.reason | type == "string" and length > 0)
      else false end)
' "$output" >/dev/null; then
    echo "failed unit contains invalid classification states or shapes" >&2
    exit 65
fi

classified=$(jq -s '[.[] | select(.classification.status == "classified")] | length' "$output")
failed=$(jq -s '[.[] | select(.classification.status == "failed")] | length' "$output")
if (( classified + failed != rows || failed == 0 )); then
    echo "failed unit is not a reconcilable partial result: classified=$classified failed=$failed rows=$rows" >&2
    exit 65
fi

jq -s '[.[] | select(.classification.status == "failed") | .entity_ref]' "$output" > "$failed_refs"
jq -c --slurpfile refs "$failed_refs" '
    select(.entity_ref as $ref | ($refs[0] | index($ref)) != null)
' "$input" > "$recovery_tmp"
recovery_rows=$(wc -l < "$recovery_tmp" | tr -d ' ')
if (( recovery_rows != failed )); then
    echo "recovery input count does not match failed identities: recovery=$recovery_rows failed=$failed" >&2
    exit 65
fi

recovery_number=1
while :; do
    recovery_unit=$(printf '%s-recovery-%03d' "$unit" "$recovery_number")
    if ! jq -e --arg unit "$recovery_unit" 'select(.unit == $unit)' "$queue" >/dev/null; then
        break
    fi
    recovery_number=$((recovery_number + 1))
done
suffix=$(printf 'recovery-%03d' "$recovery_number")
with_suffix() {
    local path=$1
    if [[ "$path" == *.* ]]; then
        printf '%s-%s.%s\n' "${path%.*}" "$suffix" "${path##*.}"
    else
        printf '%s-%s\n' "$path" "$suffix"
    fi
}
recovery_input=$(with_suffix "$input")
recovery_output=$(with_suffix "$output")
recovery_args=$(with_suffix "$(jq -r '.args' <<<"$row")")
recovery_response=$(with_suffix "$response")
recovery_log=$(with_suffix "$log")
for path in "$recovery_input" "$recovery_output" "$recovery_args" "$recovery_response" "$recovery_log"; do
    if [[ -e "$path" ]]; then
        echo "refusing to overwrite recovery artifact: $path" >&2
        exit 73
    fi
done
mkdir -p "$(dirname "$recovery_input")"
mv "$recovery_tmp" "$recovery_input"
recovery_tmp=

reserved_cost=$(jq -n --argjson rows "$rows" --argjson rate "$estimated_cost_per_chunk" '$rows * $rate')
ordinal=$(jq -s '[.[].ordinal] | max + 1' "$queue")
reconciled_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
output_sha256=$(sha256sum "$output" | cut -d' ' -f1)

jq -c --arg unit "$unit" --arg recovery_unit "$recovery_unit" \
    --arg reconciled_at "$reconciled_at" --arg output_sha256 "$output_sha256" \
    --argjson classified "$classified" --argjson failed "$failed" \
    --argjson reserved_cost "$reserved_cost" '
    if .unit == $unit then
      . + {status:"reconciled_partial",reconciled_at:$reconciled_at,
           tagged:$classified,failed:$failed,recovery_unit:$recovery_unit,
           reported_cost_usd:null,cost_reporting_complete:false,
           reserved_cost_usd:$reserved_cost,output_sha256:$output_sha256}
    else . end
' "$queue" > "$queue_tmp"

jq -cn --arg unit "$recovery_unit" --arg parent_unit "$unit" \
    --arg input "$recovery_input" --arg output "$recovery_output" \
    --arg args "$recovery_args" --arg response "$recovery_response" --arg log "$recovery_log" \
    --argjson ordinal "$ordinal" --argjson rows "$failed" \
    '{unit:$unit,ordinal:$ordinal,parent_unit:$parent_unit,rows:$rows,
      input:$input,output:$output,args:$args,response:$response,log:$log,status:"pending"}' \
    >> "$queue_tmp"
chmod --reference="$queue" "$queue_tmp"
mv -f "$queue_tmp" "$queue"
queue_tmp=

printf 'unit=%s status=reconciled_partial classified=%d failed=%d recovery_unit=%s reserved_cost_usd=%s\n' \
    "$unit" "$classified" "$failed" "$recovery_unit" "$reserved_cost" >&2
