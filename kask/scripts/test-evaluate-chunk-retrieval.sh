#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
evaluator="$repo_root/kask/scripts/audit/evaluate-chunk-retrieval.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cat > "$tmp/queries.jsonl" <<'JSONL'
{"query_id":"q1","query":"find alpha","source":"source-a.txt","evidence_quote":"alpha beta"}
{"query_id":"q2","query":"find six","source":"source-b.txt","evidence_quote":"one two three four five six"}
JSONL
cat > "$tmp/direct.jsonl" <<'JSONL'
{"entity_ref":"direct:a","source":"source-a.txt","text":"zero alpha beta gamma","word_count":4}
{"entity_ref":"direct:b","source":"source-b.txt","text":"one two three four five six","word_count":6}
JSONL
cat > "$tmp/children.jsonl" <<'JSONL'
{"entity_ref":"child:a","source":"source-a.txt","text":"alpha beta","word_count":2}
{"entity_ref":"child:b","source":"source-b.txt","text":"five six","word_count":2}
JSONL
cat > "$tmp/parents.jsonl" <<'JSONL'
{"entity_ref":"parent:a","source":"source-a.txt","text":"zero alpha beta gamma","word_count":4}
{"entity_ref":"parent:a-boundary","source":"source-a.txt","text":"boundary context only","word_count":3}
{"entity_ref":"parent:b","source":"source-b.txt","text":"one two three four five six","word_count":6}
JSONL
cat > "$tmp/map.jsonl" <<'JSONL'
{"child_ref":"child:a","source":"source-a.txt","parent_refs":["parent:a","parent:a-boundary"]}
{"child_ref":"child:b","source":"source-b.txt","parent_refs":["parent:b"]}
JSONL
: > "$tmp/direct.db"
: > "$tmp/corrupt.db"
: > "$tmp/children.db"

cat > "$tmp/fake-corpus" <<'BASH'
#!/usr/bin/env bash
set -euo pipefail
while IFS= read -r line; do
    method=$(jq -r '.method' <<<"$line")
    case "$method" in
        initialize)
            jq -cn --argjson id "$(jq '.id' <<<"$line")" '{jsonrpc:"2.0",id:$id,result:{protocolVersion:"2025-06-18",capabilities:{},serverInfo:{name:"fake",version:"1"}}}'
            ;;
        notifications/initialized)
            ;;
        tools/call)
            id=$(jq '.id' <<<"$line")
            query=$(jq -r '.params.arguments.query' <<<"$line")
            db=$(jq -r '.params.arguments.db_path' <<<"$line")
            if [[ -n ${FAKE_CALL_LOG:-} ]]; then
                printf '%s\n' "$query" >> "$FAKE_CALL_LOG"
            fi
            if [[ -n ${FAKE_FAIL_ONCE_MARKER:-} && "$query" == "find six" && ! -e "$FAKE_FAIL_ONCE_MARKER" ]]; then
                : > "$FAKE_FAIL_ONCE_MARKER"
                jq -cn --argjson id "$id" '{jsonrpc:"2.0",id:$id,result:{content:[{type:"text",text:"temporary overload"}],structuredContent:{error:"temporary overload",kind:"unavailable"},isError:true}}'
                continue
            fi
            if [[ "$db" == *children.db ]]; then
                if [[ "$query" == "find alpha" ]]; then
                    results='[{"metadata":{"entity_ref":"child:a"},"score":0.95,"text":"alpha beta"}]'
                else
                    results='[{"metadata":{"entity_ref":"child:b"},"score":0.90,"text":"five six"}]'
                fi
                total=2
            else
                if [[ "$query" == "find alpha" ]]; then
                    results='[{"metadata":{"entity_ref":"direct:a"},"score":0.95,"text":"zero alpha beta gamma"},{"metadata":{"entity_ref":"direct:b"},"score":0.40,"text":"one two three four five six"}]'
                else
                    results='[{"metadata":{"entity_ref":"direct:b"},"score":0.90,"text":"one two three four five six"}]'
                fi
                if [[ "$db" == *corrupt.db ]]; then
                    results=$(jq -c '.[0].text = "tampered retrieval text"' <<<"$results")
                fi
                total=2
            fi
            inner=$(jq -cn --arg query "$query" --argjson results "$results" --argjson total "$total" '{content:{query:$query,results:$results,total_indexed:$total}}')
            jq -cn --argjson id "$id" --arg text "$inner" '{jsonrpc:"2.0",id:$id,result:{content:[{type:"text",text:$text}],isError:false}}'
            ;;
    esac
done
BASH
chmod +x "$tmp/fake-corpus"

export HKASK_CORPUS_BINARY="$tmp/fake-corpus"
export HKASK_INFERENCE_SOCKET="test-socket"
export HKASK_EMBEDDING_MODEL="test-embedding-model"
export HKASK_INFERENCE_TIMEOUT_SECS=5

"$evaluator" direct-test direct "$tmp/queries.jsonl" "$tmp/direct.jsonl" "$tmp/direct.db" "$tmp/direct-output" 20 5
jq -e '
    .query_count == 2 and
    .source_fidelity_violations == 0 and
    .exact_evidence_recall_at_5 == 1 and
    .correct_source_recall_at_20 == 1 and
    .budget_exceeded_queries == 1 and
    .ndcg.status == "unavailable"
' "$tmp/direct-output/summary.json" >/dev/null
[[ $(wc -l < "$tmp/direct-output/raw-results.jsonl") -eq 2 ]]
[[ $(wc -l < "$tmp/direct-output/per-query-results.jsonl") -eq 2 ]]
jq -e 'select(.query_id == "q1") | .contexts[0].source == "source-a.txt" and .contexts[0].entity_ref == "direct:a"' \
    "$tmp/direct-output/per-query-results.jsonl" >/dev/null

export FAKE_FAIL_ONCE_MARKER="$tmp/fail-once-marker"
export FAKE_CALL_LOG="$tmp/resume-call-log"
if "$evaluator" resume-test direct "$tmp/queries.jsonl" "$tmp/direct.jsonl" "$tmp/direct.db" "$tmp/resume-output" 20 5; then
    echo "expected first resumable evaluation attempt to fail" >&2
    exit 1
fi
[[ $(wc -l < "$tmp/resume-output/raw-results.jsonl") -eq 1 ]]
"$evaluator" --resume resume-test direct "$tmp/queries.jsonl" "$tmp/direct.jsonl" "$tmp/direct.db" "$tmp/resume-output" 20 5
[[ $(wc -l < "$tmp/resume-output/raw-results.jsonl") -eq 2 ]]
[[ $(grep -F -x -c 'find alpha' "$FAKE_CALL_LOG") -eq 1 ]]
unset FAKE_FAIL_ONCE_MARKER FAKE_CALL_LOG

"$evaluator" small-test small-to-big "$tmp/queries.jsonl" "$tmp/children.jsonl" "$tmp/children.db" "$tmp/small-output" 20 5 "$tmp/map.jsonl" "$tmp/parents.jsonl"
jq -e '
    .query_count == 2 and
    .source_fidelity_violations == 0 and
    .exact_evidence_recall_at_5 == 1 and
    .budgeted_exact_evidence_recall == 0.5 and
    .budget_exceeded_queries == 1 and
    .small_to_big.parent_expansion_count == 3 and
    .small_to_big.boundary_crossing_children == 1 and
    .small_to_big.retrieved_children == 2 and
    .small_to_big.boundary_crossing_rate == 0.5
' "$tmp/small-output/summary.json" >/dev/null
jq -e 'select(.query_id == "q1") | .retrieval_results[0].entity_ref == "child:a" and .contexts[0].entity_ref == "parent:a" and .contexts[0].retrieved_by_child_ref == "child:a"' \
    "$tmp/small-output/per-query-results.jsonl" >/dev/null

HKASK_CORPUS_BINARY="$tmp/does-not-exist" "$evaluator" --reuse-raw "$tmp/small-output/raw-results.jsonl" \
    small-reaggregated small-to-big "$tmp/queries.jsonl" "$tmp/children.jsonl" "$tmp/children.db" \
    "$tmp/reaggregated-output" 20 5 "$tmp/map.jsonl" "$tmp/parents.jsonl"
jq -e '.query_count == 2 and .budgeted_exact_evidence_recall == 0.5 and .source_fidelity_gate == "pass"' \
    "$tmp/reaggregated-output/summary.json" >/dev/null

"$evaluator" corrupt-test direct "$tmp/queries.jsonl" "$tmp/direct.jsonl" "$tmp/corrupt.db" "$tmp/corrupt-output" 20 5
jq -e '.source_fidelity_gate == "fail" and .source_fidelity_violations == 2' \
    "$tmp/corrupt-output/summary.json" >/dev/null

if "$evaluator" direct-test direct "$tmp/queries.jsonl" "$tmp/direct.jsonl" "$tmp/direct.db" "$tmp/direct-output" 20 5 >/dev/null 2>&1; then
    echo "evaluator overwrote an existing output directory" >&2
    exit 1
fi

printf '%s\n' "evaluate chunk retrieval tests passed"
