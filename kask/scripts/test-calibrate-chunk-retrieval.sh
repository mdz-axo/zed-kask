#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
runner="$repo_root/kask/scripts/audit/calibrate-chunk-retrieval.sh"
host_call="$repo_root/kask/scripts/audit/call-corpus-tool-via-host.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

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
                    jq -n --argjson current "$(jq -c '.current_policy' <<<"$arguments")" \
                        --argjson fine "$(jq -c '.fine_policy' <<<"$arguments")" \
                        --argjson parent "$(jq -c '.parent_policy' <<<"$arguments")" \
                        '{schema_version:1,policies:{
                            reference:{engine:"hkask_memory::text_chunking::chunk_text_with_config",word_unit:"unicode_whitespace_delimited",boundary_preference:"structural_then_sentence_then_hard_max",final_remainder:"merge_backward_below_50_words",min_words:50,max_words:100,overlap_words:0,sentence_boundary:".!?"},
                            current:($current+{engine:"hkask_memory::text_chunking::chunk_text_with_config",word_unit:"unicode_whitespace_delimited",boundary_preference:"structural_then_sentence_then_hard_max",final_remainder:"retain"}),
                            fine:($fine+{engine:"hkask_memory::text_chunking::chunk_text_with_config",word_unit:"unicode_whitespace_delimited",boundary_preference:"structural_then_sentence_then_hard_max",final_remainder:"retain"}),
                            parent:($parent+{engine:"hkask_memory::text_chunking::chunk_text_with_config",word_unit:"unicode_whitespace_delimited",boundary_preference:"structural_then_sentence_then_hard_max",final_remainder:"retain"})},
                          validation:{unique_entity_refs:true,normalized_source_reconstruction:true,
                            every_child_mapped:true,every_parent_exists:true,child_map_parent_sources_agree:true}}' \
                        > "$output/manifest.json"
                    respond "$id" "$(jq -cn --arg manifest "$output/manifest.json" '{manifest:$manifest,accepted_sources:1}')"
                    ;;
                corpus_embed)
                    chunks=$(jq -r '.chunks_jsonl' <<<"$arguments")
                    db=$(jq -r '.db_path' <<<"$arguments")
                    model=$(jq -r '.model' <<<"$arguments")
                    total=$(wc -l < "$chunks" | tr -d ' ')
                    basename "$chunks" > "$db"
                    status=${FAKE_ACTUAL_MODEL_STATUS:-confirmed}
                    respond "$id" "$(jq -cn --arg model "$model" --arg actual "actual-test-embedding-model" --arg status "$status" --argjson total "$total" \
                        '{model:$model,requested_model:$model,actual_model:(if $status == "confirmed" then $actual else null end),actual_model_status:$status,identity_batches:{confirmed:(if $status == "confirmed" then 1 else 0 end),missing:(if $status == "confirmed" then 0 else 1 end)},total:$total,embedded:$total,failed:0,cancelled:false}')"
                    ;;
                corpus_query)
                    db=$(jq -r '.db_path' <<<"$arguments")
                    query=$(jq -r '.query' <<<"$arguments")
                    case "$(cat "$db")" in
                        reference.jsonl)
                            ref=calibration:e2e:reference:a:0
                            representation=reference.jsonl
                            ;;
                        current.jsonl)
                            ref=calibration:e2e:current:a:0
                            representation=current.jsonl
                            ;;
                        fine-children.jsonl)
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

export FAKE_ACTUAL_MODEL_STATUS=unavailable
if "$runner" "$tmp/run-spec.json" "$tmp/unconfirmed-run"; then
    echo "calibration accepted an unconfirmed provider model identity" >&2
    exit 1
fi
unset FAKE_ACTUAL_MODEL_STATUS

printf '%s\n' "calibrate chunk retrieval end-to-end test passed"
