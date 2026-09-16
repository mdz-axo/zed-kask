#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat >&2 <<'EOF'
usage: evaluate-chunk-retrieval.sh [--resume | --reuse-raw <raw-results-jsonl>] \
  <policy-label> <direct|small-to-big> \
  <queries-jsonl> <retrieval-representation-jsonl> <index-db> <output-dir> \
  <top-k> <word-budget> [<child-parent-map-jsonl> <parent-representation-jsonl>]
EOF
    exit 64
}

resume=false
reuse_raw=
if [[ ${1:-} == --resume ]]; then
    resume=true
    shift
elif [[ ${1:-} == --reuse-raw ]]; then
    [[ $# -ge 3 ]] || usage
    reuse_raw=$2
    shift 2
fi
if [[ $# -ne 8 && $# -ne 10 ]]; then
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
child_parent_map=${9:-}
parent_representation=${10:-}

if [[ "$mode" != direct && "$mode" != small-to-big ]]; then
    echo "mode must be direct or small-to-big" >&2
    exit 64
fi
if [[ "$mode" == direct && $# -ne 8 ]]; then
    usage
fi
if [[ "$mode" == small-to-big && $# -ne 10 ]]; then
    usage
fi
if [[ ! "$top_k" =~ ^[1-9][0-9]*$ ]] || (( top_k > 50 )); then
    echo "top-k must be an integer from 1 through 50" >&2
    exit 64
fi
if [[ ! "$word_budget" =~ ^[1-9][0-9]*$ ]]; then
    echo "word-budget must be a positive integer" >&2
    exit 64
fi
if [[ -e "$output_dir" && "$resume" != true ]]; then
    echo "refusing to overwrite output directory: $output_dir" >&2
    exit 73
fi
if [[ "$resume" == true && ! -d "$output_dir" ]]; then
    echo "resume output directory does not exist: $output_dir" >&2
    exit 66
fi
if [[ -n "$reuse_raw" && ! -f "$reuse_raw" ]]; then
    echo "reused raw-results JSONL does not exist: $reuse_raw" >&2
    exit 66
fi
for command in jq sha256sum sync; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "required command not found: $command" >&2
        exit 69
    fi
done
for path in "$queries" "$retrieval_representation" "$index_db"; do
    if [[ ! -f "$path" ]]; then
        echo "required file does not exist: $path" >&2
        exit 66
    fi
done
if [[ "$mode" == small-to-big ]]; then
    for path in "$child_parent_map" "$parent_representation"; do
        if [[ ! -f "$path" ]]; then
            echo "required file does not exist: $path" >&2
            exit 66
        fi
    done
fi

jq -s -e '
    length > 0 and
    all(.[]; (.query_id | type == "string" and length > 0) and
              (.query | type == "string" and length > 0) and
              (.source | type == "string" and length > 0) and
              (.evidence_quote | type == "string" and length > 0)) and
    ([.[].query_id] | length == (unique | length))
' "$queries" >/dev/null
jq -s -e '
    length > 0 and
    all(.[]; (.entity_ref | type == "string" and length > 0) and
              (.source | type == "string" and length > 0) and
              (.text | type == "string") and
              (.word_count | type == "number")) and
    ([.[].entity_ref] | length == (unique | length))
' "$retrieval_representation" >/dev/null
if [[ "$mode" == small-to-big ]]; then
    jq -s -e '
        length > 0 and
        all(.[]; (.child_ref | type == "string" and length > 0) and
                  (.source | type == "string" and length > 0) and
                  (.parent_refs | type == "array" and length > 0)) and
        ([.[].child_ref] | length == (unique | length))
    ' "$child_parent_map" >/dev/null
    jq -s -e '
        length > 0 and
        all(.[]; (.entity_ref | type == "string" and length > 0) and
                  (.source | type == "string" and length > 0) and
                  (.text | type == "string") and
                  (.word_count | type == "number")) and
        ([.[].entity_ref] | length == (unique | length))
    ' "$parent_representation" >/dev/null
    jq -n -e \
        --slurpfile children "$retrieval_representation" \
        --slurpfile maps "$child_parent_map" \
        --slurpfile parents "$parent_representation" '
      (reduce $maps[] as $row ({}; .[$row.child_ref] = $row)) as $maps_by_child
      | (reduce $parents[] as $row ({}; .[$row.entity_ref] = $row)) as $parents_by_ref
      | ($maps | length) == ($children | length)
        and all($children[]; . as $child
          | ($maps_by_child[$child.entity_ref] // null) as $map
          | $map != null and $map.source == $child.source
            and all($map.parent_refs[]; . as $parent_ref
              | ($parents_by_ref[$parent_ref] // null) as $parent
              | $parent != null and $parent.source == $child.source))
    ' >/dev/null || {
        echo "small-to-big source fidelity preflight failed" >&2
        exit 65
    }
fi

mkdir -p "$output_dir"
raw_results="$output_dir/raw-results.jsonl"
per_query_results="$output_dir/per-query-results.jsonl"
summary="$output_dir/summary.json"
server_log="$output_dir/server.log"
parameters="$output_dir/parameters.json"
if [[ "$resume" == true ]]; then
    for path in "$raw_results" "$server_log" "$parameters"; do
        if [[ ! -f "$path" ]]; then
            echo "resume artifact does not exist: $path" >&2
            exit 66
        fi
    done
    for path in "$per_query_results" "$summary"; do
        if [[ -e "$path" ]]; then
            echo "refusing to resume a completed evaluation: $path" >&2
            exit 73
        fi
    done
else
    if [[ -n "$reuse_raw" ]]; then
        cp "$reuse_raw" "$raw_results"
    else
        : > "$raw_results"
    fi
    : > "$server_log"
fi

host_pid=
ensure_host_pid() {
    if [[ "$host_pid" =~ ^[0-9]+$ ]]; then
        return 0
    fi
    local candidates
    candidates=$(pgrep -f '^hkask-mcp-corpus$') || true
    host_pid=${candidates%%$'\n'*}
    if [[ ! "$host_pid" =~ ^[0-9]+$ ]]; then
        echo "running host-managed hkask-mcp-corpus process not found" >&2
        return 69
    fi
}
read_host_env() {
    local name=$1
    ensure_host_pid || return $?
    tr '\0' '\n' < "/proc/$host_pid/environ" | sed -n "s/^${name}=//p"
}

if [[ -z ${HKASK_EMBEDDING_MODEL:-} ]]; then
    if [[ -n "$reuse_raw" ]]; then
        echo "offline raw reuse requires HKASK_EMBEDDING_MODEL for provenance" >&2
        exit 69
    fi
    HKASK_EMBEDDING_MODEL=$(read_host_env HKASK_EMBEDDING_MODEL)
    export HKASK_EMBEDDING_MODEL
fi
if [[ -z ${HKASK_EMBEDDING_MODEL:-} ]]; then
    echo "host embedding model is unavailable" >&2
    exit 69
fi

query_count=$(wc -l < "$queries" | tr -d ' ')
chunk_count=$(wc -l < "$retrieval_representation" | tr -d ' ')
index_bytes=$(stat -c %s "$index_db")
index_sha256=$(sha256sum "$index_db" | cut -d' ' -f1)
query_sha256=$(sha256sum "$queries" | cut -d' ' -f1)
representation_sha256=$(sha256sum "$retrieval_representation" | cut -d' ' -f1)
child_parent_map_sha256=
parent_representation_sha256=
if [[ "$mode" == small-to-big ]]; then
    child_parent_map_sha256=$(sha256sum "$child_parent_map" | cut -d' ' -f1)
    parent_representation_sha256=$(sha256sum "$parent_representation" | cut -d' ' -f1)
fi
parameter_embedding_model=$HKASK_EMBEDDING_MODEL
started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
if [[ "$resume" == true ]]; then
    jq -e \
        --arg policy "$policy" --arg mode "$mode" --arg queries "$queries" \
        --arg query_sha256 "$query_sha256" --arg representation "$retrieval_representation" \
        --arg representation_sha256 "$representation_sha256" --arg index_db "$index_db" \
        --arg index_sha256 "$index_sha256" --arg embedding_model "$parameter_embedding_model" \
        --arg child_parent_map_sha256 "$child_parent_map_sha256" \
        --arg parent_representation_sha256 "$parent_representation_sha256" \
        --argjson query_count "$query_count" --argjson chunk_count "$chunk_count" \
        --argjson index_bytes "$index_bytes" --argjson top_k "$top_k" \
        --argjson word_budget "$word_budget" '
          .policy == $policy and .mode == $mode and .queries == $queries and
          .query_sha256 == $query_sha256 and .retrieval_representation == $representation and
          .representation_sha256 == $representation_sha256 and .index_db == $index_db and
          .index_sha256 == $index_sha256 and
          .child_parent_map_sha256 == $child_parent_map_sha256 and
          .parent_representation_sha256 == $parent_representation_sha256 and
          .embedding_model == $embedding_model and
          .query_count == $query_count and .chunk_count == $chunk_count and
          .index_bytes == $index_bytes and .top_k == $top_k and .word_budget == $word_budget
        ' "$parameters" >/dev/null || {
            echo "resume parameters do not match the recorded evaluation" >&2
            exit 65
        }
else
    jq -n \
        --arg policy "$policy" \
        --arg mode "$mode" \
        --arg queries "$queries" \
        --arg query_sha256 "$query_sha256" \
        --arg representation "$retrieval_representation" \
        --arg representation_sha256 "$representation_sha256" \
        --arg index_db "$index_db" \
        --arg index_sha256 "$index_sha256" \
        --arg child_parent_map_sha256 "$child_parent_map_sha256" \
        --arg parent_representation_sha256 "$parent_representation_sha256" \
        --arg started_at "$started_at" \
        --arg embedding_model "$parameter_embedding_model" \
        --argjson query_count "$query_count" \
        --argjson chunk_count "$chunk_count" \
        --argjson index_bytes "$index_bytes" \
        --argjson top_k "$top_k" \
        --argjson word_budget "$word_budget" \
        '{policy:$policy,mode:$mode,queries:$queries,query_sha256:$query_sha256,
          retrieval_representation:$representation,representation_sha256:$representation_sha256,
          child_parent_map_sha256:$child_parent_map_sha256,
          parent_representation_sha256:$parent_representation_sha256,
          index_db:$index_db,index_sha256:$index_sha256,index_bytes:$index_bytes,chunk_count:$chunk_count,
          query_count:$query_count,top_k:$top_k,word_budget:$word_budget,
          embedding_model:$embedding_model,started_at:$started_at}' > "$parameters"
fi

completed_ids=$(mktemp)
if [[ -s "$raw_results" ]]; then
    jq -s -e --slurpfile queries "$queries" '
        ([.[].query_id] | length == (unique | length)) and
        all(.[]; .query_id as $id | any($queries[]; .query_id == $id))
    ' "$raw_results" >/dev/null
    jq -r '.query_id' "$raw_results" > "$completed_ids"
fi
completed=$(wc -l < "$completed_ids" | tr -d ' ')

if [[ -z "$reuse_raw" ]]; then
    binary=${HKASK_CORPUS_BINARY:-$HOME/.local/bin/hkask-mcp-corpus}
    if [[ ! -f "$binary" ]]; then
        echo "corpus MCP binary does not exist: $binary" >&2
        exit 66
    fi

if [[ -z ${HKASK_CORPUS_BINARY:-} || -z ${HKASK_INFERENCE_SOCKET:-} || -z ${HKASK_EMBEDDING_MODEL:-} ]]; then

    HKASK_INFERENCE_SOCKET=$(read_host_env HKASK_INFERENCE_SOCKET)
    HKASK_INFERENCE_TIMEOUT_SECS=$(read_host_env HKASK_INFERENCE_TIMEOUT_SECS)
    HKASK_EMBEDDING_MODEL=$(read_host_env HKASK_EMBEDDING_MODEL)
    DEEPINFRA_TOKEN=$(read_host_env DEEPINFRA_TOKEN)
    export HKASK_INFERENCE_SOCKET HKASK_INFERENCE_TIMEOUT_SECS HKASK_EMBEDDING_MODEL DEEPINFRA_TOKEN
fi
if [[ -z ${HKASK_INFERENCE_SOCKET:-} || -z ${HKASK_EMBEDDING_MODEL:-} ]]; then
    echo "corpus inference configuration is incomplete" >&2
    exit 69
fi

unset HKASK_DB_PASSPHRASE
response_timeout=${HKASK_CALIBRATION_RESPONSE_TIMEOUT_SECS:-$(( ${HKASK_INFERENCE_TIMEOUT_SECS:-600} + 30 ))}
if [[ ! "$response_timeout" =~ ^[1-9][0-9]*$ ]]; then
    echo "response timeout must be a positive integer" >&2
    exit 64
fi

coproc CORPUS_MCP { "$binary" 2>>"$server_log"; }
out_fd=${CORPUS_MCP[0]}
in_fd=${CORPUS_MCP[1]}
corpus_pid=$CORPUS_MCP_PID
cleanup() {
    rm -f "${completed_ids:-}"
    if [[ -n ${in_fd:-} ]]; then
        eval "exec ${in_fd}>&-" 2>/dev/null || true
        in_fd=
    fi
    if [[ -n ${corpus_pid:-} ]]; then
        wait "$corpus_pid" 2>/dev/null || true
        corpus_pid=
    fi
}
trap cleanup EXIT

call_response=
read_response() {
    local expected_id=$1
    local line
    call_response=
    while IFS= read -r -t "$response_timeout" line <&"$out_fd"; do
        if jq -e --argjson id "$expected_id" '.id == $id' <<<"$line" >/dev/null 2>&1; then
            call_response=$line
            return 0
        fi
    done
    echo "corpus MCP ended or timed out before response id $expected_id" >&2
    return 70
}

printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"chunk-retrieval-calibration","version":"1"}}}' >&"$in_fd"
read_response 1
jq -e '.result.protocolVersion == "2025-06-18" and .error == null' <<<"$call_response" >/dev/null
printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' >&"$in_fd"

request_id=2
while IFS= read -r query_row; do
    query_id=$(jq -r '.query_id' <<<"$query_row")
    if grep -Fx -m 1 -- "$query_id" "$completed_ids" >/dev/null; then
        ((request_id += 1))
        continue
    fi
    request=$(jq -cn \
        --argjson id "$request_id" \
        --argjson query_row "$query_row" \
        --arg db_path "$index_db" \
        --argjson top_k "$top_k" \
        '{jsonrpc:"2.0",id:$id,method:"tools/call",params:{name:"corpus_query",arguments:{
          query:$query_row.query,top_k:$top_k,generate_answer:false,include_text:true,
          min_score:0,db_path:$db_path}}}')
    printf '%s\n' "$request" >&"$in_fd"
    read_response "$request_id"
    if jq -e '.result.isError == true or .error != null' <<<"$call_response" >/dev/null; then
        failure_path="$output_dir/failed-response-$request_id-$(date -u +%Y%m%dT%H%M%S).json"
        printf '%s\n' "$call_response" > "$failure_path"
        echo "corpus_query failed at query id $query_id" >&2
        exit 1
    fi
    tool_content=$(jq -cer '
        [.result.content[]? | select(.type == "text") | .text | fromjson | .content] as $content
        | if ($content | length) == 1 and ($content[0].results | type) == "array"
          then $content[0]
          else error("unexpected corpus_query tool envelope")
          end
    ' <<<"$call_response")
    jq -cn --argjson query_spec "$query_row" --argjson retrieval "$tool_content" \
        '{query_id:$query_spec.query_id,query_spec:$query_spec,retrieval:$retrieval}' >> "$raw_results"
    sync -d "$raw_results"
    printf '%s\n' "$query_id" >> "$completed_ids"
    ((completed += 1))
    printf 'policy=%s completed=%d/%d query_id=%s\n' "$policy" "$completed" "$query_count" "$query_id" >&2
    ((request_id += 1))
done < "$queries"

cleanup
trap - EXIT
else
    printf 'reused_raw_results=%s queries=%d\n' "$reuse_raw" "$completed" >&2
fi
rm -f "$completed_ids"

if (( completed != query_count )); then
    echo "query reconciliation failed: completed=$completed planned=$query_count" >&2
    exit 65
fi
if ! jq -s -e --argjson expected "$chunk_count" '
    length > 0 and all(.[]; .retrieval.total_indexed == $expected)
' "$raw_results" >/dev/null; then
    echo "index hydration count does not match retrieval representation count" >&2
    exit 65
fi

transform_program=$(mktemp)
trap 'rm -f "$transform_program"' EXIT
cat > "$transform_program" <<'JQ'
def word_count($text): [$text | scan("\\S+")] | length;
def duplicate_stats($contexts):
  [$contexts[] | .text as $text | ($text | [scan("\\S+")]) as $words
   | if ($words | length) >= 6
     then range(0; ($words | length) - 5) as $i
          | ($words[$i:$i+6] | join(" ") | ascii_downcase)
     else empty end] as $grams
  | ($grams | length) as $total
  | ($grams | unique | length) as $unique
  | {total_sixgrams:$total, duplicate_sixgrams:($total - $unique),
     rate:(if $total == 0 then 0 else (($total - $unique) / $total) end)};
def admit($contexts; $budget):
  reduce $contexts[] as $context
    ({contexts:[],retrieved_words:0};
     ($context.word_count // word_count($context.text)) as $words
     | if (.retrieved_words + $words) <= $budget
       then .contexts += [$context + {context_rank:((.contexts | length) + 1),word_count:$words}]
            | .retrieved_words += $words
       else . end)
  | .budget_exceeded = (.retrieved_words > $budget);

def direct_result($raw; $rows; $budget):
  [$raw.retrieval.results | to_entries[]
   | (.key + 1) as $rank
   | .value as $hit
   | ($hit.metadata.entity_ref // "") as $ref
   | ($rows[$ref] // null) as $row
   | {rank:$rank,entity_ref:$ref,score:$hit.score,text:($hit.text // ""),
      source:($row.source // null),word_count:($row.word_count // word_count($hit.text // "")),
      violations:((if $row == null then [{kind:"unknown_retrieval_ref",entity_ref:$ref}] else [] end)
        + (if $row != null and ($hit.text // null) != $row.text
           then [{kind:"retrieved_text_mismatch",entity_ref:$ref}] else [] end))}] as $hits
  | admit($hits; $budget) as $admitted
  | $admitted.contexts as $contexts
  | {query_id:$raw.query_id,query:$raw.query_spec.query,expected_source:$raw.query_spec.source,
     evidence_quote:$raw.query_spec.evidence_quote,retrieval_results:$hits,contexts:$contexts,
     rank_metrics:{
       first_exact_rank:(([$hits[] | select(.text | contains($raw.query_spec.evidence_quote)) | .rank] | first) // null),
       first_correct_source_rank:(([$hits[] | select(.source == $raw.query_spec.source) | .rank] | first) // null)},
     budget_metrics:{
       exact_evidence_found:(if $admitted.budget_exceeded then false else any($contexts[]; .text | contains($raw.query_spec.evidence_quote)) end),
       correct_source_found:(if $admitted.budget_exceeded then false else any($contexts[]; .source == $raw.query_spec.source) end),
       strict_budget_satisfied:($admitted.budget_exceeded | not)},
     retrieved_words:$admitted.retrieved_words,budget_exceeded:$admitted.budget_exceeded,
     duplicate_overlap:duplicate_stats($contexts),
     source_fidelity_violations:[$hits[].violations[]?],
     total_indexed:$raw.retrieval.total_indexed};
def small_result($raw; $children_by_ref; $maps_by_child; $parents_by_ref; $budget):
  [$raw.retrieval.results | to_entries[]
   | (.key + 1) as $rank
   | .value as $hit
   | ($hit.metadata.entity_ref // "") as $ref
   | ($children_by_ref[$ref] // null) as $child
   | ($maps_by_child[$ref] // null) as $mapping
   | {rank:$rank,entity_ref:$ref,score:$hit.score,text:($hit.text // ""),
      source:($child.source // null),parent_refs:($mapping.parent_refs // []),
      map_source:($mapping.source // null),
      violations:((if $child == null then [{kind:"unknown_child_ref",entity_ref:$ref}] else [] end)
        + (if $child != null and ($hit.text // null) != $child.text
           then [{kind:"retrieved_child_text_mismatch",entity_ref:$ref}] else [] end)
        + (if $mapping == null then [{kind:"missing_child_parent_map",entity_ref:$ref}] else [] end)
        + (if $child != null and $mapping != null and $child.source != $mapping.source
           then [{kind:"child_map_source_mismatch",entity_ref:$ref}] else [] end))}] as $children
  | (reduce $children[] as $child
      ({seen:{},contexts:[]};
       reduce $child.parent_refs[] as $parent_ref
         (.; if .seen[$parent_ref] then .
             else ($parents_by_ref[$parent_ref] // null) as $parent
             | .seen[$parent_ref] = true
             | .contexts += [{entity_ref:$parent_ref,source:($parent.source // null),
                 text:($parent.text // ""),word_count:($parent.word_count // 0),
                 retrieved_by_child_rank:$child.rank,retrieved_by_child_ref:$child.entity_ref,
                 score:$child.score,rank:((.contexts | length) + 1),
                 violations:((if $parent == null then [{kind:"unknown_parent_ref",entity_ref:$parent_ref}] else [] end)
                   + (if $parent != null and $child.source != $parent.source
                      then [{kind:"child_parent_source_mismatch",entity_ref:$parent_ref,child_ref:$child.entity_ref}] else [] end))}]
             end)) | .contexts) as $expanded
  | admit($expanded; $budget) as $admitted
  | $admitted.contexts as $contexts
  | {query_id:$raw.query_id,query:$raw.query_spec.query,expected_source:$raw.query_spec.source,
     evidence_quote:$raw.query_spec.evidence_quote,retrieval_results:$children,
     expanded_contexts:$expanded,contexts:$contexts,
     rank_metrics:{
       first_exact_rank:(([$expanded[] | select(.text | contains($raw.query_spec.evidence_quote)) | .rank] | first) // null),
       first_correct_source_rank:(([$expanded[] | select(.source == $raw.query_spec.source) | .rank] | first) // null)},
     budget_metrics:{
       exact_evidence_found:(if $admitted.budget_exceeded then false else any($contexts[]; .text | contains($raw.query_spec.evidence_quote)) end),
       correct_source_found:(if $admitted.budget_exceeded then false else any($contexts[]; .source == $raw.query_spec.source) end),
       strict_budget_satisfied:($admitted.budget_exceeded | not)},
     retrieved_words:$admitted.retrieved_words,budget_exceeded:$admitted.budget_exceeded,
     duplicate_overlap:duplicate_stats($contexts),
     source_fidelity_violations:([$children[].violations[]?] + [$expanded[].violations[]?]),
     small_to_big:{retrieved_children:($children | length),
       boundary_crossing_children:([$children[] | select((.parent_refs | length) > 1)] | length),
       parent_expansion_count:($expanded | length),admitted_parent_count:($contexts | length)},
     total_indexed:$raw.retrieval.total_indexed};

(reduce $retrieval_rows[] as $row ({}; .[$row.entity_ref] = $row)) as $retrieval_by_ref
| if $mode == "direct" then
    $raw_rows[] | direct_result(.; $retrieval_by_ref; $budget)
  else
    (reduce $map_rows[] as $row ({}; .[$row.child_ref] = $row)) as $maps_by_child
    | (reduce $parent_rows[] as $row ({}; .[$row.entity_ref] = $row)) as $parents_by_ref
    | $raw_rows[] | small_result(.; $retrieval_by_ref; $maps_by_child; $parents_by_ref; $budget)
  end
JQ

if [[ "$mode" == direct ]]; then
    jq -cn --arg mode "$mode" --argjson budget "$word_budget" \
        --slurpfile raw_rows "$raw_results" \
        --slurpfile retrieval_rows "$retrieval_representation" \
        --argjson map_rows '[]' --argjson parent_rows '[]' \
        -f "$transform_program" > "$per_query_results"
else
    jq -cn --arg mode "$mode" --argjson budget "$word_budget" \
        --slurpfile raw_rows "$raw_results" \
        --slurpfile retrieval_rows "$retrieval_representation" \
        --slurpfile map_rows "$child_parent_map" \
        --slurpfile parent_rows "$parent_representation" \
        -f "$transform_program" > "$per_query_results"
fi
sync -d "$per_query_results"

jq -s \
    --arg policy "$policy" \
    --arg mode "$mode" \
    --arg model "$parameter_embedding_model" \
    --arg query_sha256 "$query_sha256" \
    --arg representation_sha256 "$representation_sha256" \
    --arg index_sha256 "$index_sha256" \
    --arg child_parent_map_sha256 "$child_parent_map_sha256" \
    --arg parent_representation_sha256 "$parent_representation_sha256" \
    --arg completed_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --argjson top_k "$top_k" \
    --argjson word_budget "$word_budget" \
    --argjson index_bytes "$index_bytes" \
    --argjson chunk_count "$chunk_count" '
  def ratio($numerator; $denominator): if $denominator == 0 then 0 else $numerator / $denominator end;
  def hit_count($field; $k): [.[] | select(.rank_metrics[$field] != null and .rank_metrics[$field] <= $k)] | length;
  length as $count
  | ([.[].source_fidelity_violations | length] | add // 0) as $violations
  | ([.[].duplicate_overlap.total_sixgrams] | add // 0) as $sixgrams
  | ([.[].duplicate_overlap.duplicate_sixgrams] | add // 0) as $duplicate_sixgrams
  | {
      policy:$policy,mode:$mode,query_count:$count,top_k:$top_k,word_budget:$word_budget,
      embedding_model:$model,query_sha256:$query_sha256,
      representation_sha256:$representation_sha256,index_sha256:$index_sha256,
      child_parent_map_sha256:$child_parent_map_sha256,
      parent_representation_sha256:$parent_representation_sha256,
      index_bytes:$index_bytes,chunk_count:$chunk_count,
      source_fidelity_gate:(if $violations == 0 then "pass" else "fail" end),
      source_fidelity_violations:$violations,
      exact_evidence_recall_at_5:ratio(hit_count("first_exact_rank"; 5); $count),
      exact_evidence_recall_at_20:ratio(hit_count("first_exact_rank"; 20); $count),
      correct_source_recall_at_5:ratio(hit_count("first_correct_source_rank"; 5); $count),
      correct_source_recall_at_20:ratio(hit_count("first_correct_source_rank"; 20); $count),
      exact_evidence_mrr:ratio(([.[] | if .rank_metrics.first_exact_rank == null then 0 else 1 / .rank_metrics.first_exact_rank end] | add // 0); $count),
      correct_source_mrr:ratio(([.[] | if .rank_metrics.first_correct_source_rank == null then 0 else 1 / .rank_metrics.first_correct_source_rank end] | add // 0); $count),
      budgeted_exact_evidence_recall:ratio(([.[] | select(.budget_metrics.exact_evidence_found)] | length); $count),
      budgeted_correct_source_recall:ratio(([.[] | select(.budget_metrics.correct_source_found)] | length); $count),
      retrieved_words:{total:([.[].retrieved_words] | add // 0),
        mean:ratio(([.[].retrieved_words] | add // 0); $count),max:([.[].retrieved_words] | max // 0)},
      budget_exceeded_queries:([.[] | select(.budget_exceeded)] | length),
      duplicate_overlap:{total_sixgrams:$sixgrams,duplicate_sixgrams:$duplicate_sixgrams,
        rate:ratio($duplicate_sixgrams; $sixgrams)},
      ndcg:{status:"unavailable",reason:"No exhaustive or graded relevance judgments exist for the candidate corpus; observed exact/source hits cannot define an unbiased ideal DCG."},
      answer_grounding:{status:"unavailable",reason:"The fixed retriever evaluation does not generate answers; grounding is unavailable unless a separate answer-and-citation measurement is run."},
      completed_at:$completed_at
    }
    + (if $mode == "small-to-big" then
        ([.[].small_to_big.retrieved_children] | add // 0) as $children
        | ([.[].small_to_big.boundary_crossing_children] | add // 0) as $boundary
        | {small_to_big:{retrieved_children:$children,boundary_crossing_children:$boundary,
             boundary_crossing_rate:ratio($boundary; $children),
             parent_expansion_count:([.[].small_to_big.parent_expansion_count] | add // 0),
             admitted_parent_count:([.[].small_to_big.admitted_parent_count] | add // 0)}}
       else {} end)
' "$per_query_results" > "$summary"
sync -d "$summary"
rm -f "$transform_program"
trap - EXIT

source_fidelity_gate=$(jq -r '.source_fidelity_gate' "$summary")
printf 'policy=%s queries=%d source_fidelity_gate=%s source_fidelity_violations=%s summary=%s\n' \
    "$policy" "$completed" "$source_fidelity_gate" \
    "$(jq -r '.source_fidelity_violations' "$summary")" "$summary" >&2
if [[ "$source_fidelity_gate" != pass ]]; then
    exit 65
fi
