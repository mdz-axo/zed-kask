#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
executor="$repo_root/kask/scripts/audit/run-corpus-classification-queue.sh"
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

printf '%s\n' "corpus classification queue tests passed"
