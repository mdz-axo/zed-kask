#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
tmp=$(mktemp -d)
trap 'chmod -R u+w "$tmp" 2>/dev/null || true; rm -rf "$tmp"' EXIT
fixture_root="$tmp/shared-repo"
mkdir -p "$fixture_root/kask/scripts/audit"
for script in calibrate-chunk-retrieval.sh build-chunk-calibration-queries.sh \
    call-corpus-tool-via-host.sh evaluate-chunk-retrieval.sh inspect-chunk-calibration-run.sh; do
    cp "$repo_root/kask/scripts/audit/$script" "$fixture_root/kask/scripts/audit/$script"
done
runner="$fixture_root/kask/scripts/audit/calibrate-chunk-retrieval.sh"
host_call="$fixture_root/kask/scripts/audit/call-corpus-tool-via-host.sh"

cat > "$tmp/source-a.txt" <<'TEXT'
Alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega joins source faithful calibration evidence across every deterministic representation policy.
TEXT
digest=$(sha256sum "$tmp/source-a.txt" | cut -d' ' -f1)
cat > "$tmp/run-spec.json" <<JSON
{
  "accepted_sources": [{
    "source": "source-a.txt",
    "raw_path": "$tmp/source-a.txt",
    "raw_sha256": "$digest",
    "canonical_path": "$tmp/source-a.txt",
    "canonical_sha256": "$digest"
  }],
  "entity_ref_prefix": "calibration:e2e",
  "embedding_model": "requested-test-embedding-model",
  "batch_size": 4,
  "embedding_max_concurrency": 4,
  "embedding_shard_retry_limit": 3,
  "embedding_retry_backoff_secs": 0,
  "max_queries": 1,
  "retriever": {"name":"corpus_query_cosine","top_k":5,"word_budget":200,"min_score":0},
  "selection": {"max_budgeted_exact_evidence_loss_count":1},
  "policies": {
    "current": {"min_words":10,"max_words":80,"overlap_words":0,"sentence_boundary":".!?"},
    "fine": {"min_words":5,"max_words":20,"overlap_words":0,"sentence_boundary":".!?"},
    "parent": {"min_words":10,"max_words":100,"overlap_words":0,"sentence_boundary":".!?"}
  }
}
JSON

cat > "$tmp/fake-corpus" <<'BASH'
#!/usr/bin/env bash
set -euo pipefail
respond() {
    local id=$1 payload=$2
    inner=$(jq -cn --argjson content "$payload" '{content:$content}')
    jq -cn --argjson id "$id" --arg text "$inner" \
        '{jsonrpc:"2.0",id:$id,result:{content:[{type:"text",text:$text}],isError:false}}'
}
maybe_mutate_shared_runtime() {
    if [[ -n ${FAKE_MUTATE_SHARED_ONCE_MARKER:-} && ! -e $FAKE_MUTATE_SHARED_ONCE_MARKER ]]; then
        touch "$FAKE_MUTATE_SHARED_ONCE_MARKER"
        printf '%s\n' '# shared runtime identity drift' >> "$FAKE_MUTATE_SHARED_PATH"
    fi
}
store_inventory() {
    local db=$1 chunks=$2 model=$3 state new merged
    state="${db}.inventory.jsonl"
    new=$(mktemp)
    merged=$(mktemp)
    jq -c --arg model "$model" '{entity_ref:.entity_ref,model:$model}' "$chunks" > "$new"
    if [[ -f "$state" ]]; then
        jq -sc 'flatten | reduce .[] as $row ({}; .[$row.entity_ref] = $row) | [.[]][]' "$state" "$new" > "$merged"
    else
        cat "$new" > "$merged"
    fi
    mv "$merged" "$state"
    rm -f "$new"
    printf 'indexed\n' > "$db"
}
while IFS= read -r line; do
    method=$(jq -r '.method' <<<"$line")
    case "$method" in
        initialize)
            jq -cn --argjson id "$(jq '.id' <<<"$line")" \
                '{jsonrpc:"2.0",id:$id,result:{protocolVersion:"2025-06-18",capabilities:{},serverInfo:{name:"fake",version:"1"}}}'
            ;;
        notifications/initialized)
            ;;
        tools/call)
            id=$(jq '.id' <<<"$line")
            name=$(jq -r '.params.name' <<<"$line")
            arguments=$(jq -c '.params.arguments' <<<"$line")
            case "$name" in
                corpus_build_chunk_representations)
                    output=$(jq -r '.output_dir' <<<"$arguments")
                    source=$(jq -r '.accepted_sources[0].source' <<<"$arguments")
                    canonical=$(jq -r '.accepted_sources[0].canonical_path' <<<"$arguments")
                    raw=$(jq -r '.accepted_sources[0].raw_path' <<<"$arguments")
                    canonical_sha=$(jq -r '.accepted_sources[0].canonical_sha256' <<<"$arguments")
                    raw_sha=$(jq -r '.accepted_sources[0].raw_sha256' <<<"$arguments")
                    text=$(cat "$canonical")
                    first=$(awk '{for(i=1;i<=15;i++) printf "%s%s", (i==1?"":" "), $i}' "$canonical")
                    rest=$(awk '{for(i=16;i<=NF;i++) printf "%s%s", (i==16?"":" "), $i}' "$canonical")
                    mkdir -p "$output"
                    provenance=$(jq -cn --arg raw_path "$raw" --arg raw_sha256 "$raw_sha" \
                        --arg canonical_path "$canonical" --arg canonical_sha256 "$canonical_sha" \
                        '{raw_path:$raw_path,raw_sha256:$raw_sha256,canonical_path:$canonical_path,
                          canonical_sha256:$canonical_sha256,canonical_normalization:"split_whitespace_join_single_space"}')
                    jq -cn --arg source "$source" --arg text "$text" --argjson provenance "$provenance" \
                        '{entity_ref:"calibration:e2e:reference:a:0",source:$source,text:$text,
                          word_count:([$text|scan("\\S+")]|length),provenance:$provenance}' > "$output/reference.jsonl"
                    jq -cn --arg source "$source" --arg text "$text" --argjson provenance "$provenance" \
                        '{entity_ref:"calibration:e2e:current:a:0",source:$source,text:$text,
                          word_count:([$text|scan("\\S+")]|length),provenance:$provenance}' > "$output/current.jsonl"
                    jq -cn --arg source "$source" --arg text "$first" --argjson provenance "$provenance" \
                        '{entity_ref:"calibration:e2e:fine:a:0",source:$source,text:$text,
                          word_count:([$text|scan("\\S+")]|length),provenance:$provenance}' > "$output/fine-children.jsonl"
                    jq -cn --arg source "$source" --arg text "$rest" --argjson provenance "$provenance" \
                        '{entity_ref:"calibration:e2e:fine:a:1",source:$source,text:$text,
                          word_count:([$text|scan("\\S+")]|length),provenance:$provenance}' >> "$output/fine-children.jsonl"
                    jq -cn --arg source "$source" --arg text "$text" --argjson provenance "$provenance" \
                        '{entity_ref:"calibration:e2e:parent:a:0",source:$source,text:$text,
                          word_count:([$text|scan("\\S+")]|length),provenance:$provenance}' > "$output/parents.jsonl"
                    for child in calibration:e2e:fine:a:0 calibration:e2e:fine:a:1; do
                        jq -cn --arg child "$child" --arg source "$source" --argjson provenance "$provenance" \
                            '{child_ref:$child,source:$source,parent_refs:["calibration:e2e:parent:a:0"],provenance:$provenance}'
                    done > "$output/child-parent-map.jsonl"
                    jq -n --arg source "$source" \
                        --argjson current "$(jq -c '.current_policy' <<<"$arguments")" \
                        --argjson fine "$(jq -c '.fine_policy' <<<"$arguments")" \
                        --argjson parent "$(jq -c '.parent_policy' <<<"$arguments")" \
                        '{schema_version:2,
                          boilerplate_exclusion_reports:{($source):{input_words:30,retained_words:30,exclusions:[]}},
                          policies:{
                            reference:{engine:"hkask_memory::text_chunking::chunk_text_with_config",word_unit:"unicode_whitespace_delimited",boundary_preference:"structural_then_sentence_then_hard_max",final_remainder:"merge_backward_below_50_words",min_words:50,max_words:100,overlap_words:0,sentence_boundary:".!?"},
                            current:($current+{engine:"hkask_memory::text_chunking::chunk_text_with_config",word_unit:"unicode_whitespace_delimited",boundary_preference:"structural_then_sentence_then_hard_max",final_remainder:"retain"}),
                            fine:($fine+{engine:"hkask_memory::text_chunking::chunk_text_with_config",word_unit:"unicode_whitespace_delimited",boundary_preference:"structural_then_sentence_then_hard_max",final_remainder:"retain"}),
                            parent:($parent+{engine:"hkask_memory::text_chunking::chunk_text_with_config",word_unit:"unicode_whitespace_delimited",boundary_preference:"structural_then_sentence_then_hard_max",final_remainder:"retain"})},
                          validation:{accepted_source_count:1,boilerplate_filter_applied:true,
                            unique_entity_refs:true,normalized_source_reconstruction:true,
                            every_child_mapped:true,every_parent_exists:true,child_map_parent_sources_agree:true}}' \
                        > "$output/manifest.json"
                    respond "$id" "$(jq -cn --arg manifest "$output/manifest.json" '{manifest:$manifest,accepted_sources:1}')"
                    ;;
                corpus_embed)
                    chunks=$(jq -r '.chunks_jsonl' <<<"$arguments")
                    db=$(jq -r '.db_path' <<<"$arguments")
                    model=$(jq -r '.model' <<<"$arguments")
                    actual=${FAKE_ACTUAL_MODEL:-actual-test-embedding-model}
                    total=$(wc -l < "$chunks" | tr -d ' ')
                    if [[ -n ${FAKE_EMBED_CALL_LOG:-} ]]; then
                        printf '%s\t%s\t%s\t%s\n' "$(basename "$db" .db)" "$(stat -c %s "$chunks")" \
                            "${HKASK_MAX_CONCURRENCY:-missing}" "$(jq -r '.entity_ref' "$chunks" | paste -sd, -)" >> "$FAKE_EMBED_CALL_LOG"
                    fi
                    status=${FAKE_ACTUAL_MODEL_STATUS:-confirmed}
                    if [[ -n ${FAKE_EMBED_INTERRUPT_DB:-} && $(basename "$db" .db) == "$FAKE_EMBED_INTERRUPT_DB" && -n ${FAKE_EMBED_INTERRUPT_MARKER:-} && ! -e $FAKE_EMBED_INTERRUPT_MARKER ]]; then
                        partial=$(mktemp)
                        head -1 "$chunks" > "$partial"
                        store_inventory "$db" "$partial" "$actual"
                        rm -f "$partial"
                        touch "$FAKE_EMBED_INTERRUPT_MARKER"
                        exit 0
                    fi
                    if [[ -n ${FAKE_EMBED_FAIL_ONCE_MARKER:-} && ! -e $FAKE_EMBED_FAIL_ONCE_MARKER ]]; then
                        touch "$FAKE_EMBED_FAIL_ONCE_MARKER"
                        failed_refs=$(jq -sc '.[0:1] | map(.entity_ref)' "$chunks")
                        successful=$(mktemp)
                        tail -n +2 "$chunks" > "$successful"
                        if [[ -s "$successful" ]]; then
                            store_inventory "$db" "$successful" "$actual"
                        else
                            printf 'indexed\n' > "$db"
                        fi
                        rm -f "$successful"
                        respond "$id" "$(jq -cn --arg model "$model" --arg actual "$actual" --arg status "$status" --argjson total "$total" --argjson refs "$failed_refs" \
                            '{model:$model,requested_model:$model,actual_model:(if $status == "confirmed" then $actual else null end),actual_model_status:$status,identity_batches:{confirmed:(if $status == "confirmed" then 1 else 0 end),missing:(if $status == "confirmed" then 0 else 1 end)},total:$total,embedded:($total-1),failed:1,failed_entity_refs:$refs,failed_entity_refs_complete:true,cancelled:false}')"
                        maybe_mutate_shared_runtime
                        continue
                    fi
                    store_inventory "$db" "$chunks" "$actual"
                    respond "$id" "$(jq -cn --arg model "$model" --arg actual "$actual" --arg status "$status" --argjson total "$total" \
                        '{model:$model,requested_model:$model,actual_model:(if $status == "confirmed" then $actual else null end),actual_model_status:$status,identity_batches:{confirmed:(if $status == "confirmed" then 1 else 0 end),missing:(if $status == "confirmed" then 0 else 1 end)},total:$total,embedded:$total,failed:0,failed_entity_refs:[],failed_entity_refs_complete:true,cancelled:false}')"
                    maybe_mutate_shared_runtime
                    ;;
                corpus_embedding_inventory)
                    chunks=$(jq -r '.chunks_jsonl' <<<"$arguments")
                    db=$(jq -r '.db_path' <<<"$arguments")
                    expected=$(jq -r '.expected_model' <<<"$arguments")
                    state="${db}.inventory.jsonl"
                    [[ -f "$state" ]] || : > "$state"
                    inventory=$(jq -n --arg expected "$expected" --slurpfile requested "$chunks" --slurpfile stored "$state" '
                      ($requested | map(.entity_ref) | unique | sort) as $refs
                      | ($stored | reduce .[] as $row ({}; .[$row.entity_ref] = $row.model)) as $models
                      | [$refs[] | select($models[.] == null)] as $missing
                      | [$refs[] | select($models[.] != null and $models[.] != $expected)
                          | {entity_ref:.,stored_model:$models[.]}] as $mismatched
                      | (($missing + [$mismatched[].entity_ref]) | unique | sort) as $retry
                      | {requested:($refs|length),stored_matching_model:([$refs[] | select($models[.] == $expected)]|length),
                         missing_entity_refs:$missing,mismatched_model_entity_refs:$mismatched,
                         retry_entity_refs:$retry,complete:($retry|length == 0),expected_model:$expected}')
                    respond "$id" "$inventory"
                    ;;
                corpus_query)
                    db=$(jq -r '.db_path' <<<"$arguments")
                    query=$(jq -r '.query' <<<"$arguments")
                    case "$(basename "$db")" in
                        reference.db)
                            ref=calibration:e2e:reference:a:0
                            representation=reference.jsonl
                            ;;
                        current.db)
                            ref=calibration:e2e:current:a:0
                            representation=current.jsonl
                            ;;
                        fine.db)
                            ref=calibration:e2e:fine:a:0
                            representation=fine-children.jsonl
                            ;;
                        *) exit 65 ;;
                    esac
                    rows=$(dirname "$db")/representations/$representation
                    text=$(jq -r --arg ref "$ref" 'select(.entity_ref == $ref) | .text' "$rows")
                    total=$(wc -l < "$rows" | tr -d ' ')
                    results=$(jq -cn --arg ref "$ref" --arg text "$text" \
                        '[{metadata:{entity_ref:$ref},score:0.99,text:$text}]')
                    respond "$id" "$(jq -cn --arg query "$query" --argjson results "$results" --argjson total "$total" \
                        '{query:$query,results:$results,total_indexed:$total}')"
                    ;;
                corpus_tag_chunks)
                    chunks=$(jq -r '.chunks_jsonl' <<<"$arguments")
                    output=$(jq -r '.output' <<<"$arguments")
                    total=$(wc -l < "$chunks" | tr -d ' ')
                    jq -c '. + {classification:{status:"classified",ontology_protocol:"published-term-resolution-v1"},candidate_terms:["one","two","three"],ontology_tags:{core:["5w1h_core"]},concepts:["5w1h_core"]}' "$chunks" > "$output"
                    respond "$id" "$(jq -cn --argjson total "$total" '{total_chunks:$total,tagged:$total,failed:0,reported_cost_usd:0.001,cost_reporting_complete:true}')"
                    ;;
                *)
                    jq -cn --argjson id "$id" '{jsonrpc:"2.0",id:$id,error:{code:-32601,message:"unsupported"}}'
                    ;;
            esac
            ;;
    esac
done
BASH
chmod +x "$tmp/fake-corpus"

export HKASK_CORPUS_BINARY="$tmp/fake-corpus"
export HKASK_INFERENCE_SOCKET="test-socket"
export HKASK_INFERENCE_TIMEOUT_SECS=5
export HKASK_EMBEDDING_MODEL="requested-test-embedding-model"
export HKASK_CLASSIFIER_MODEL="requested-test-classifier-model"
export HKASK_TEMPLATE_ROOT="$tmp"
export HKASK_CALIBRATION_RESPONSE_TIMEOUT_SECS=10
export HKASK_CALIBRATION_EMBED_SHARD_MAX_BYTES=1000
export FAKE_EMBED_CALL_LOG="$tmp/embed-calls.log"
export FAKE_EMBED_FAIL_ONCE_MARKER="$tmp/embed-failed-once"

printf '%s\n' '{"entity_ref":"test:tag:0","source":"source-a.txt","text":"alpha beta","word_count":2}' > "$tmp/tag-input.jsonl"
jq -n --arg input "$tmp/tag-input.jsonl" --arg output "$tmp/tag-output.jsonl" \
    '{chunks_jsonl:$input,output:$output,concurrency:1,tag_batch_size:1,dry_run:false}' > "$tmp/tag-args.json"
"$host_call" corpus_tag_chunks "$tmp/tag-args.json" "$tmp/tag-response.json" "$tmp/tag.log"
[[ $(wc -l < "$tmp/tag-output.jsonl") -eq 1 ]]
jq -e '.classification.status == "classified" and .classification.ontology_protocol == "published-term-resolution-v1"' "$tmp/tag-output.jsonl" >/dev/null

"$runner" "$tmp/run-spec.json" "$tmp/run"
jq -e '
  (.requested_embedding_model == "requested-test-embedding-model") and
  (.actual_embedding_model == "actual-test-embedding-model") and
  (.eligible_policies | sort) == ["current","fine","reference"] and
  .max_budgeted_exact_evidence_loss_count == 1 and
  (.best_budgeted_exact_evidence_hit_count | type) == "number" and
  (.material_contenders | length) > 0 and
  (.policies | length) == 3 and
  all(.policies[]; .source_fidelity_gate == "pass") and
  all(.policies[]; .ndcg.status == "unavailable") and
  all(.policies[]; .answer_grounding.status == "unavailable") and
  (.measured_costs.embedding | length) == 3 and
  any(.measured_costs.embedding[]; .attempted_rows > .total_rows) and
  (.measured_costs.evaluation | length) == 3
' "$tmp/run/comparison.json" >/dev/null
jq -e '
  (.run_id | test("^[0-9a-f]{64}$")) and
  (.requested_embedding_model == "requested-test-embedding-model") and
  (.actual_embedding_model == "actual-test-embedding-model") and
  all([.representations.reference,.representations.current,.representations.fine,
       .representations.child_parent_map,.representations.parent,
       .indexes.reference,.indexes.current,.indexes.fine][]; test("^[0-9a-f]{64}$"))
' "$tmp/run/run-identity.json" >/dev/null
for policy in reference current fine; do
    [[ -s "$tmp/run/$policy.db" ]]
    [[ $(wc -l < "$tmp/run/evaluation-$policy/raw-results.jsonl") -eq 1 ]]
done
awk -F '\t' '$2 > 1000 || $3 != 4 { exit 1 }' "$FAKE_EMBED_CALL_LOG"
[[ $(awk -F '\t' '$1 == "reference" { count++ } END { print count + 0 }' "$FAKE_EMBED_CALL_LOG") -gt 1 ]]
[[ $(awk -F '\t' '$1 == "fine" { count++ } END { print count + 0 }' "$FAKE_EMBED_CALL_LOG") -gt 1 ]]

cat "$tmp/source-a.txt" > "$tmp/dirty-source.txt"
printf '%s\n' 'OceanofPDF.com' >> "$tmp/dirty-source.txt"
dirty_digest=$(sha256sum "$tmp/dirty-source.txt" | cut -d' ' -f1)
jq --arg path "$tmp/dirty-source.txt" --arg digest "$dirty_digest" \
    '.entity_ref_prefix = "calibration:e2e-dirty"
     | .accepted_sources[0].raw_path = $path
     | .accepted_sources[0].canonical_path = $path
     | .accepted_sources[0].raw_sha256 = $digest
     | .accepted_sources[0].canonical_sha256 = $digest' \
    "$tmp/run-spec.json" > "$tmp/dirty-run-spec.json"
embed_calls_before_dirty=$(if [[ -f "$FAKE_EMBED_CALL_LOG" ]]; then wc -l < "$FAKE_EMBED_CALL_LOG"; else echo 0; fi)
if "$runner" "$tmp/dirty-run-spec.json" "$tmp/dirty-run" \
    >"$tmp/dirty-run.stdout" 2>"$tmp/dirty-run.stderr"; then
    echo "calibration accepted a representation containing a distribution watermark" >&2
    exit 1
fi
grep -F 'representation retains forbidden watermark or boilerplate' "$tmp/dirty-run.stderr" >/dev/null
[[ $(wc -l < "$FAKE_EMBED_CALL_LOG") -eq "$embed_calls_before_dirty" ]]
[[ ! -e "$tmp/dirty-run/reference.db" ]]
[[ ! -e "$tmp/dirty-run/run-identity.json" ]]

cp "$host_call" "$tmp/shared-host-call.original"
export FAKE_MUTATE_SHARED_ONCE_MARKER="$tmp/shared-runtime-mutated"
export FAKE_MUTATE_SHARED_PATH="$host_call"
"$runner" "$tmp/run-spec.json" "$tmp/runtime-capsule-run"
[[ -s "$tmp/runtime-capsule-run/runtime/manifest.json" ]]
[[ -s "$tmp/runtime-capsule-run/runtime/manifest.json.sha256" ]]
[[ $(sha256sum "$tmp/runtime-capsule-run/runtime/manifest.json" | cut -d' ' -f1) == "$(cat "$tmp/runtime-capsule-run/runtime/manifest.json.sha256")" ]]
[[ $(sha256sum "$host_call" | cut -d' ' -f1) != "$(sha256sum "$tmp/shared-host-call.original" | cut -d' ' -f1)" ]]
jq -e '
  .schema_version == 1 and
  (.components.runner.sha256 | test("^[0-9a-f]{64}$")) and
  (.components.query_builder.sha256 | test("^[0-9a-f]{64}$")) and
  (.components.host_call.sha256 | test("^[0-9a-f]{64}$")) and
  (.components.evaluator.sha256 | test("^[0-9a-f]{64}$")) and
  (.components.inspector.sha256 | test("^[0-9a-f]{64}$")) and
  (.components.corpus_binary.sha256 | test("^[0-9a-f]{64}$"))
' "$tmp/runtime-capsule-run/runtime/manifest.json" >/dev/null
mv "$tmp/shared-host-call.original" "$host_call"
chmod +x "$host_call"
unset FAKE_MUTATE_SHARED_ONCE_MARKER FAKE_MUTATE_SHARED_PATH

costs_before_resume=$(sha256sum "$tmp/run/measured-costs.json" | cut -d' ' -f1)
"$runner" --resume "$tmp/run-spec.json" "$tmp/run"
[[ $(sha256sum "$tmp/run/measured-costs.json" | cut -d' ' -f1) == "$costs_before_resume" ]]
cp "$tmp/run/evaluation-current/summary.json" "$tmp/current-summary.json"
jq '.source_fidelity_gate = "fail"' "$tmp/current-summary.json" > "$tmp/run/evaluation-current/summary.json"
if "$runner" --resume "$tmp/run-spec.json" "$tmp/run"; then
    echo "resume accepted a tampered completed evaluation summary" >&2
    exit 1
fi
mv "$tmp/current-summary.json" "$tmp/run/evaluation-current/summary.json"
jq '.retriever.top_k = 4' "$tmp/run-spec.json" > "$tmp/changed-spec.json"
if "$runner" --resume "$tmp/changed-spec.json" "$tmp/run"; then
    echo "resume accepted changed run identity" >&2
    exit 1
fi

unset FAKE_EMBED_FAIL_ONCE_MARKER
export HKASK_CALIBRATION_EMBED_SHARD_MAX_BYTES=2000
export FAKE_EMBED_INTERRUPT_DB=fine
export FAKE_EMBED_INTERRUPT_MARKER="$tmp/embed-interrupted-once"
: > "$FAKE_EMBED_CALL_LOG"
if "$runner" "$tmp/run-spec.json" "$tmp/interrupted-run"; then
    echo "calibration unexpectedly completed after a lost shard response" >&2
    exit 1
fi
[[ -s "$tmp/interrupted-run/run-preseal-identity.json" ]]
[[ ! -e "$tmp/interrupted-run/run-identity.json" ]]
[[ -s "$tmp/interrupted-run/embed/checkpoints/reference-shard-00000.json" ]]
[[ -s "$tmp/interrupted-run/embed/checkpoints/current-shard-00000.json" ]]
[[ ! -e "$tmp/interrupted-run/embed/checkpoints/fine-shard-00000.json" ]]
[[ -e "$tmp/interrupted-run/embed/fine-shard-00000-attempt-01-response.json" ]]
[[ ! -s "$tmp/interrupted-run/embed/fine-shard-00000-attempt-01-response.json" ]]
reference_calls_before=$(awk -F '\t' '$1 == "reference" { count++ } END { print count + 0 }' "$FAKE_EMBED_CALL_LOG")
current_calls_before=$(awk -F '\t' '$1 == "current" { count++ } END { print count + 0 }' "$FAKE_EMBED_CALL_LOG")

export FAKE_ACTUAL_MODEL=actual-test-other-model
if "$runner" --resume "$tmp/run-spec.json" "$tmp/interrupted-run"; then
    echo "resume accepted a changed provider-confirmed actual model" >&2
    exit 1
fi
unset FAKE_ACTUAL_MODEL
"$runner" --resume "$tmp/run-spec.json" "$tmp/interrupted-run"
[[ $(awk -F '\t' '$1 == "reference" { count++ } END { print count + 0 }' "$FAKE_EMBED_CALL_LOG") -eq "$reference_calls_before" ]]
[[ $(awk -F '\t' '$1 == "current" { count++ } END { print count + 0 }' "$FAKE_EMBED_CALL_LOG") -eq "$current_calls_before" ]]
last_fine_refs=$(awk -F '\t' '$1 == "fine" { refs=$4 } END { print refs }' "$FAKE_EMBED_CALL_LOG")
[[ "$last_fine_refs" == "calibration:e2e:fine:a:1" ]]
jq -e '
  [.embedding[] | select(.policy == "fine")][0]
  | .attempted_rows == 4 and .recovered_rows == 1 and .attempts == 3
' "$tmp/interrupted-run/measured-costs.json" >/dev/null
for checkpoint in "$tmp/interrupted-run/embed/checkpoints"/*.json; do
    [[ $(sha256sum "$checkpoint" | cut -d' ' -f1) == "$(cat "$checkpoint.sha256")" ]]
done

checkpoint="$tmp/interrupted-run/embed/checkpoints/current-shard-00000.json"
cp "$checkpoint" "$tmp/checkpoint.original"
jq '.embedded_rows = 0' "$tmp/checkpoint.original" > "$checkpoint"
if "$runner" --resume "$tmp/run-spec.json" "$tmp/interrupted-run"; then
    echo "resume accepted a tampered shard checkpoint" >&2
    exit 1
fi
mv "$tmp/checkpoint.original" "$checkpoint"

capsule_binary="$tmp/interrupted-run/runtime/bin/hkask-mcp-corpus"
cp "$capsule_binary" "$tmp/capsule-binary.original"
chmod u+w "$capsule_binary"
printf '%s\n' '# capsule binary identity drift' >> "$capsule_binary"
embed_calls_before_capsule_tamper=$(wc -l < "$FAKE_EMBED_CALL_LOG")
if "$runner" --resume "$tmp/run-spec.json" "$tmp/interrupted-run"; then
    echo "resume accepted a changed runtime capsule binary" >&2
    exit 1
fi
[[ $(wc -l < "$FAKE_EMBED_CALL_LOG") -eq "$embed_calls_before_capsule_tamper" ]]
chmod u+w "$(dirname "$capsule_binary")"
mv "$tmp/capsule-binary.original" "$capsule_binary"
chmod 0555 "$capsule_binary"
chmod 0755 "$(dirname "$capsule_binary")"

representation="$tmp/interrupted-run/representations/current.jsonl"
cp "$representation" "$tmp/current-representation.original"
printf '\n' >> "$representation"
if "$runner" --resume "$tmp/run-spec.json" "$tmp/interrupted-run"; then
    echo "resume accepted a changed representation" >&2
    exit 1
fi
mv "$tmp/current-representation.original" "$representation"

cp "$tmp/source-a.txt" "$tmp/source-a.original"
printf '%s\n' 'source identity drift' >> "$tmp/source-a.txt"
if "$runner" --resume "$tmp/run-spec.json" "$tmp/interrupted-run"; then
    echo "resume accepted a changed accepted source" >&2
    exit 1
fi
mv "$tmp/source-a.original" "$tmp/source-a.txt"
unset FAKE_EMBED_INTERRUPT_DB FAKE_EMBED_INTERRUPT_MARKER

export FAKE_ACTUAL_MODEL_STATUS=unavailable
if "$runner" "$tmp/run-spec.json" "$tmp/unconfirmed-run"; then
    echo "calibration accepted an unconfirmed provider model identity" >&2
    exit 1
fi
unset FAKE_ACTUAL_MODEL_STATUS

printf '%s\n' "calibrate chunk retrieval end-to-end test passed"
