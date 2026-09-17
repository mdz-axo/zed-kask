#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
executor="$repo_root/kask/scripts/audit/run-corpus-classification-queue.sh"
reconciler="$repo_root/kask/scripts/audit/reconcile-corpus-classification-unit.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/inputs" "$tmp/outputs" "$tmp/args" "$tmp/responses" "$tmp/logs"

cat > "$tmp/inputs/unit-000.jsonl" <<'JSONL'
{"entity_ref":"test:0","source":"a.txt","text":"alpha","word_count":1}
{"entity_ref":"test:1","source":"a.txt","text":"beta","word_count":1}
JSONL
cat > "$tmp/inputs/unit-001.jsonl" <<'JSONL'
{"entity_ref":"test:2","source":"b.txt","text":"gamma","word_count":1}
JSONL
for ordinal in 0 1; do
    unit=$(printf 'unit-%03d' "$ordinal")
    rows=2
    [[ "$ordinal" -eq 1 ]] && rows=1
    jq -cn --arg unit "$unit" --arg input "$tmp/inputs/$unit.jsonl" --arg output "$tmp/outputs/$unit.jsonl" --arg args "$tmp/args/$unit.json" --arg response "$tmp/responses/$unit.json" --arg log "$tmp/logs/$unit.log" --argjson ordinal "$ordinal" --argjson rows "$rows" '{unit:$unit,ordinal:$ordinal,rows:$rows,input:$input,output:$output,args:$args,response:$response,log:$log,status:"pending"}' >> "$tmp/queue.jsonl"
done

cat > "$tmp/fake-runner" <<'BASH'
#!/usr/bin/env bash
set -euo pipefail
tool=$1
args_file=$2
response=$3
log=$4
[[ "$tool" == corpus_tag_chunks ]]
chunks=$(jq -r '.chunks_jsonl' "$args_file")
output=$(jq -r '.output' "$args_file")
rows=$(wc -l < "$chunks" | tr -d ' ')
jq -c '. + {classification:{status:"classified",ontology_protocol:"published-term-resolution-v1"},candidate_terms:["term one","term two","term three"],ontology_tags:{core:["5w1h_core"]},concepts:["5w1h_core"]}' "$chunks" > "$output"
cost=$(jq -n --argjson rows "$rows" '$rows * 0.001')
inner=$(jq -cn --argjson rows "$rows" --argjson cost "$cost" '{content:{total_chunks:$rows,tagged:$rows,failed:0,planned_batches:1,provider_responses:1,successful_response_usage:{prompt_tokens:1,completion_tokens:1,total_tokens:2},reported_cost_usd:$cost,cost_reporting_complete:true,repaired_outer_arrays:0}}')
jq -cn --arg text "$inner" '{jsonrpc:"2.0",id:2,result:{content:[{type:"text",text:$text}],isError:false}}' > "$response"
printf '%s\n' ok > "$log"
printf '%s\n' "$chunks" >> "$FAKE_CALL_LOG"
BASH
chmod +x "$tmp/fake-runner"
export FAKE_CALL_LOG="$tmp/calls.log"

"$executor" "$tmp/queue.jsonl" "$tmp/fake-runner" 1.0 2 10 0.001 1
jq -s -e 'length == 2 and ([.[] | select(.status == "completed")] | length) == 1 and ([.[] | select(.status == "pending")] | length) == 1' "$tmp/queue.jsonl" >/dev/null
[[ $(wc -l < "$tmp/calls.log") -eq 1 ]]
"$executor" "$tmp/queue.jsonl" "$tmp/fake-runner" 1.0 2 10 0.001
jq -s -e 'length == 2 and all(.[]; .status == "completed" and .failed == 0 and .cost_reporting_complete == true)' "$tmp/queue.jsonl" >/dev/null
[[ $(wc -l < "$tmp/calls.log") -eq 2 ]]
[[ $(wc -l < "$tmp/outputs/unit-000.jsonl") -eq 2 ]]
[[ $(wc -l < "$tmp/outputs/unit-001.jsonl") -eq 1 ]]
"$executor" "$tmp/queue.jsonl" "$tmp/fake-runner" 1.0 2 10 0.001
[[ $(wc -l < "$tmp/calls.log") -eq 2 ]]

mkdir -p "$tmp/low/inputs" "$tmp/low/outputs" "$tmp/low/args" "$tmp/low/responses" "$tmp/low/logs"
cp "$tmp/inputs/unit-000.jsonl" "$tmp/low/inputs/unit-000.jsonl"
jq -cn --arg input "$tmp/low/inputs/unit-000.jsonl" --arg output "$tmp/low/outputs/unit-000.jsonl" --arg args "$tmp/low/args/unit-000.json" --arg response "$tmp/low/responses/unit-000.json" --arg log "$tmp/low/logs/unit-000.log" '{unit:"unit-000",ordinal:0,rows:2,input:$input,output:$output,args:$args,response:$response,log:$log,status:"pending"}' > "$tmp/low/queue.jsonl"
if "$executor" "$tmp/low/queue.jsonl" "$tmp/fake-runner" 0.001 2 10 0.001; then
    echo "expected cost gate to block the queue" >&2
    exit 1
fi
[[ ! -e "$tmp/low/outputs/unit-000.jsonl" ]]
jq -e '.status == "pending"' "$tmp/low/queue.jsonl" >/dev/null

mkdir -p "$tmp/partial/inputs" "$tmp/partial/outputs" "$tmp/partial/args" "$tmp/partial/responses" "$tmp/partial/logs"
cat > "$tmp/partial/inputs/unit-000.jsonl" <<'JSONL'
{"entity_ref":"partial:0","source":"a.txt","text":"alpha","word_count":1}
{"entity_ref":"partial:1","source":"a.txt","text":"beta","word_count":1}
{"entity_ref":"partial:2","source":"a.txt","text":"gamma","word_count":1}
JSONL
cat > "$tmp/partial/outputs/unit-000.jsonl" <<'JSONL'
{"entity_ref":"partial:0","source":"a.txt","text":"alpha","word_count":1,"classification":{"status":"classified","ontology_protocol":"published-term-resolution-v1"},"candidate_terms":["one","two","three"],"ontology_tags":{"core":["5w1h_core"]},"concepts":["5w1h_core"]}
{"entity_ref":"partial:1","source":"a.txt","text":"beta","word_count":1,"classification":{"status":"failed","reason":"temporary route loss"},"candidate_terms":[],"ontology_tags":{},"concepts":[]}
{"entity_ref":"partial:2","source":"a.txt","text":"gamma","word_count":1,"classification":{"status":"failed","reason":"temporary route loss"},"candidate_terms":[],"ontology_tags":{},"concepts":[]}
JSONL
printf '%s\n' '{"result":"partial"}' > "$tmp/partial/responses/unit-000.json"
printf '%s\n' 'partial failure' > "$tmp/partial/logs/unit-000.log"
jq -cn --arg input "$tmp/partial/inputs/unit-000.jsonl" --arg output "$tmp/partial/outputs/unit-000.jsonl" --arg args "$tmp/partial/args/unit-000.json" --arg response "$tmp/partial/responses/unit-000.json" --arg log "$tmp/partial/logs/unit-000.log" '{unit:"unit-000",ordinal:0,rows:3,input:$input,output:$output,args:$args,response:$response,log:$log,status:"failed_classification"}' > "$tmp/partial/queue.jsonl"
"$reconciler" "$tmp/partial/queue.jsonl" unit-000 0.001
jq -s -e 'length == 2 and .[0].status == "reconciled_partial" and .[0].tagged == 1 and .[0].failed == 2 and .[0].reserved_cost_usd == 0.003 and .[1].status == "pending" and .[1].rows == 2 and .[1].parent_unit == "unit-000"' "$tmp/partial/queue.jsonl" >/dev/null
recovery_input=$(jq -r 'select(.parent_unit == "unit-000") | .input' "$tmp/partial/queue.jsonl")
[[ $(wc -l < "$recovery_input") -eq 2 ]]
[[ $(jq -r '.entity_ref' "$recovery_input" | sort | tr '\n' ' ') == 'partial:1 partial:2 ' ]]
"$executor" "$tmp/partial/queue.jsonl" "$tmp/fake-runner" 1.0 2 10 0.001 1
jq -s -e 'length == 2 and .[0].status == "reconciled_partial" and .[1].status == "completed" and .[1].tagged == 2' "$tmp/partial/queue.jsonl" >/dev/null

mkdir -p "$tmp/mixed/inputs" "$tmp/mixed/outputs" "$tmp/mixed/args" "$tmp/mixed/responses" "$tmp/mixed/logs"
printf '%s\n' \
    '{"entity_ref":"mixed:0","source":"a.txt","text":"alpha","word_count":1}' \
    '{"entity_ref":"mixed:1","source":"a.txt","text":"beta","word_count":1}' \
    > "$tmp/mixed/inputs/unit-000.jsonl"
printf '%s\n' \
    '{"entity_ref":"mixed:2","source":"b.txt","text":"gamma","word_count":1}' \
    > "$tmp/mixed/inputs/unit-001.jsonl"
for ordinal in 0 1; do
    unit=$(printf 'unit-%03d' "$ordinal")
    rows=$((2 - ordinal))
    jq -cn --arg unit "$unit" --arg input "$tmp/mixed/inputs/$unit.jsonl" --arg output "$tmp/mixed/outputs/$unit.jsonl" --arg args "$tmp/mixed/args/$unit.json" --arg response "$tmp/mixed/responses/$unit.json" --arg log "$tmp/mixed/logs/$unit.log" --argjson ordinal "$ordinal" --argjson rows "$rows" '{unit:$unit,ordinal:$ordinal,rows:$rows,input:$input,output:$output,args:$args,response:$response,log:$log,status:"pending"}' >> "$tmp/mixed/queue.jsonl"
done
cat > "$tmp/mixed-runner" <<'BASH'
#!/usr/bin/env bash
set -euo pipefail
args_file=$2
response=$3
log=$4
chunks=$(jq -r '.chunks_jsonl' "$args_file")
output=$(jq -r '.output' "$args_file")
rows=$(wc -l < "$chunks" | tr -d ' ')
failed=0
if [[ "$chunks" == *unit-000.jsonl ]]; then
    failed=1
fi
jq -c --argjson failed "$failed" 'if $failed == 1 and .entity_ref == "mixed:1" then . + {classification:{status:"failed",reason:"synthetic model-contract rejection"}} else . + {classification:{status:"classified",ontology_protocol:"published-term-resolution-v1"},candidate_terms:["term one","term two","term three"],ontology_tags:{core:["5w1h_core"]},concepts:["5w1h_core"]} end' "$chunks" > "$output"
tagged=$((rows - failed))
cost=$(jq -n --argjson rows "$rows" '$rows * 0.001')
inner=$(jq -cn --argjson rows "$rows" --argjson tagged "$tagged" --argjson failed "$failed" --argjson cost "$cost" '{content:{total_chunks:$rows,tagged:$tagged,failed:$failed,reported_cost_usd:$cost,cost_reporting_complete:true}}')
jq -cn --arg text "$inner" '{jsonrpc:"2.0",id:2,result:{content:[{type:"text",text:$text}],isError:false}}' > "$response"
printf '%s\n' ok > "$log"
printf '%s\n' "$chunks" >> "$MIXED_CALL_LOG"
BASH
chmod +x "$tmp/mixed-runner"
export MIXED_CALL_LOG="$tmp/mixed-calls.log"
if "$executor" "$tmp/mixed/queue.jsonl" "$tmp/mixed-runner" 1.0 2 10 0.001 2; then
    echo "expected mixed wave to return a terminal-partial checkpoint" >&2
    exit 1
fi
[[ $(wc -l < "$tmp/mixed-calls.log") -eq 2 ]]
jq -s -e '.[0].status == "failed_classification" and .[0].tagged == 1 and .[0].failed == 1 and .[1].status == "completed" and .[1].tagged == 1' "$tmp/mixed/queue.jsonl" >/dev/null

printf '%s\n' "corpus classification queue tests passed"
