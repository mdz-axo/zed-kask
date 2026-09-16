#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "usage: $0 [--resume] <run-spec-json> <output-dir>" >&2
    exit 64
}

resume=false
if [[ ${1:-} == --resume ]]; then
    resume=true
    shift
fi
[[ $# -eq 2 ]] || usage
run_spec=$1
output_dir=$2
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
query_builder="$repo_root/kask/scripts/audit/build-chunk-calibration-queries.sh"
host_call="$repo_root/kask/scripts/audit/call-corpus-tool-via-host.sh"
evaluator="$repo_root/kask/scripts/audit/evaluate-chunk-retrieval.sh"

for command in jq sha256sum stat date cmp; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "required command not found: $command" >&2
        exit 69
    fi
done
if [[ ! -f "$run_spec" ]]; then
    echo "run spec does not exist: $run_spec" >&2
    exit 66
fi
for script in "$query_builder" "$host_call" "$evaluator"; do
    if [[ ! -x "$script" ]]; then
        echo "required executable script does not exist: $script" >&2
        exit 66
    fi
done
jq -e '
  (.accepted_sources | type == "array" and length > 0) and
  (.entity_ref_prefix | type == "string" and length > 0) and
  (.embedding_model | type == "string" and length > 0) and
  (.batch_size | type == "number" and . > 0 and floor == .) and
  (.max_queries | type == "number" and . > 0 and floor == .) and
  (.retriever.name == "corpus_query_cosine") and
  (.retriever.top_k | type == "number" and . > 0 and . <= 50 and floor == .) and
  (.retriever.word_budget | type == "number" and . > 0 and floor == .) and
  (.retriever.min_score == 0) and
  all([.policies.current,.policies.fine,.policies.parent][];
    (.min_words | type == "number" and . > 0 and floor == .) and
    (.max_words | type == "number" and . > 0 and floor == .) and
    (.overlap_words | type == "number" and . >= 0 and floor == .) and
    (.sentence_boundary | type == "string" and length > 0))
' "$run_spec" >/dev/null || {
    echo "run spec does not satisfy the calibration contract" >&2
    exit 64
}

if [[ "$resume" == true ]]; then
    if [[ ! -d "$output_dir" || ! -f "$output_dir/run-identity.json" ]]; then
        echo "resume requires an output directory with run-identity.json" >&2
        exit 66
    fi
else
    if [[ -e "$output_dir" ]]; then
        echo "refusing to overwrite output directory: $output_dir" >&2
        exit 73
    fi
    mkdir -p "$output_dir"
fi

queries="$output_dir/queries.jsonl"
representations_dir="$output_dir/representations"
representation_manifest="$representations_dir/manifest.json"
reference_representation="$representations_dir/reference.jsonl"
current_representation="$representations_dir/current.jsonl"
fine_representation="$representations_dir/fine-children.jsonl"
parent_representation="$representations_dir/parents.jsonl"
child_parent_map="$representations_dir/child-parent-map.jsonl"
reference_db="$output_dir/reference.db"
current_db="$output_dir/current.db"
fine_db="$output_dir/fine.db"
run_identity="$output_dir/run-identity.json"
costs="$output_dir/measured-costs.json"
comparison="$output_dir/comparison.json"

sha_file() {
    sha256sum "$1" | cut -d' ' -f1
}
now_ns() {
    date +%s%N
}
elapsed_ms() {
    local start=$1
    local end=$2
    echo $(( (end - start) / 1000000 ))
}
tool_content() {
    jq -cer '
      [.result.content[]? | select(.type == "text") | .text | fromjson | .content] as $content
      | if ($content | length) == 1 then $content[0]
        else error("unexpected corpus tool envelope") end
    ' "$1"
}
verify_accepted_sources() {
    local row source raw_path canonical_path expected_raw expected_canonical actual_raw actual_canonical
    while IFS= read -r row; do
        source=$(jq -r '.source' <<<"$row")
        raw_path=$(jq -r '.raw_path' <<<"$row")
        canonical_path=$(jq -r '.canonical_path' <<<"$row")
        expected_raw=$(jq -r '.raw_sha256 | ascii_downcase' <<<"$row")
        expected_canonical=$(jq -r '.canonical_sha256 | ascii_downcase' <<<"$row")
        for path in "$raw_path" "$canonical_path"; do
            if [[ ! -f "$path" ]]; then
                echo "accepted source file does not exist: $path" >&2
                return 66
            fi
        done
        actual_raw=$(sha_file "$raw_path")
        actual_canonical=$(sha_file "$canonical_path")
        if [[ "$actual_raw" != "$expected_raw" || "$actual_canonical" != "$expected_canonical" ]]; then
            echo "accepted source identity changed: $source" >&2
            return 65
        fi
    done < <(jq -c '.accepted_sources[]' "$run_spec")
}

verify_accepted_sources
accepted_sources_sha256=$(jq -cS '.accepted_sources' "$run_spec" | sha256sum | cut -d' ' -f1)
run_spec_sha256=$(jq -cS . "$run_spec" | sha256sum | cut -d' ' -f1)
evaluator_sha256=$(sha_file "$evaluator")

if [[ "$resume" != true ]]; then
    max_queries=$(jq -r '.max_queries' "$run_spec")
    build_start=$(now_ns)
    "$query_builder" "$run_spec" "$queries" "$max_queries"

    jq -n --slurpfile spec "$run_spec" --arg output_dir "$representations_dir" '
      {accepted_sources:$spec[0].accepted_sources,
       output_dir:$output_dir,
       entity_ref_prefix:$spec[0].entity_ref_prefix,
       current_policy:$spec[0].policies.current,
       fine_policy:$spec[0].policies.fine,
       parent_policy:$spec[0].policies.parent}
    ' > "$output_dir/build-representations-arguments.json"
    "$host_call" corpus_build_chunk_representations \
        "$output_dir/build-representations-arguments.json" \
        "$output_dir/build-representations-response.json" \
        "$output_dir/build-representations-server.log"
    tool_content "$output_dir/build-representations-response.json" >/dev/null
    build_end=$(now_ns)

    for path in "$representation_manifest" "$reference_representation" "$current_representation" \
        "$fine_representation" "$parent_representation" "$child_parent_map"; do
        if [[ ! -f "$path" ]]; then
            echo "representation tool did not publish required artifact: $path" >&2
            exit 65
        fi
    done
    jq -e '
      .validation.unique_entity_refs and
      .validation.normalized_source_reconstruction and
      .validation.every_child_mapped and
      .validation.every_parent_exists and
      .validation.child_map_parent_sources_agree
    ' "$representation_manifest" >/dev/null

    requested_model=$(jq -r '.embedding_model' "$run_spec")
    batch_size=$(jq -r '.batch_size' "$run_spec")
    export HKASK_EMBEDDING_MODEL=$requested_model
    mkdir -p "$output_dir/embed"
    embed_cost_rows="$output_dir/embed-costs.jsonl"
    : > "$embed_cost_rows"
    actual_model=
    for policy in reference current fine; do
        case "$policy" in
            reference) representation=$reference_representation; index_db=$reference_db ;;
            current) representation=$current_representation; index_db=$current_db ;;
            fine) representation=$fine_representation; index_db=$fine_db ;;
        esac
        arguments="$output_dir/embed/$policy-arguments.json"
        response="$output_dir/embed/$policy-response.json"
        log="$output_dir/embed/$policy-server.log"
        jq -n \
            --arg chunks_jsonl "$representation" \
            --arg db_path "$index_db" \
            --arg model "$requested_model" \
            --argjson batch_size "$batch_size" \
            '{chunks_jsonl:$chunks_jsonl,tagged_jsonl:null,db_path:$db_path,
              model:$model,batch_size:$batch_size}' > "$arguments"
        started=$(now_ns)
        "$host_call" corpus_embed "$arguments" "$response" "$log"
        ended=$(now_ns)
        content=$(tool_content "$response")
        reported_requested_model=$(jq -er '.requested_model | select(type == "string" and length > 0)' <<<"$content")
        if [[ "$reported_requested_model" != "$requested_model" ]]; then
            echo "embedding transport changed requested model identity for $policy: $reported_requested_model" >&2
            exit 65
        fi
        actual_model_status=$(jq -er '.actual_model_status | select(type == "string")' <<<"$content")
        if [[ "$actual_model_status" != "confirmed" ]]; then
            echo "embedding provider did not confirm one model identity for $policy: $actual_model_status" >&2
            exit 65
        fi
        model=$(jq -er '.actual_model | select(type == "string" and length > 0)' <<<"$content")
        embedded=$(jq -er '.embedded' <<<"$content")
        failed=$(jq -er '.failed' <<<"$content")
        total=$(jq -er '.total' <<<"$content")
        if [[ "$failed" -ne 0 || "$embedded" -ne "$total" ]]; then
            echo "embedding reconciliation failed for $policy" >&2
            exit 65
        fi
        if [[ -z "$actual_model" ]]; then
            actual_model=$model
        elif [[ "$actual_model" != "$model" ]]; then
            echo "embedding policies used different actual models: $actual_model vs $model" >&2
            exit 65
        fi
        if [[ ! -f "$index_db" ]]; then
            echo "corpus_embed did not create index: $index_db" >&2
            exit 65
        fi
        jq -cn --arg policy "$policy" --arg model "$model" \
            --argjson elapsed_ms "$(elapsed_ms "$started" "$ended")" \
            --argjson total "$total" --argjson embedded "$embedded" \
            --argjson index_bytes "$(stat -c %s "$index_db")" \
            '{policy:$policy,actual_embedding_model:$model,elapsed_ms:$elapsed_ms,
              total_rows:$total,embedded_rows:$embedded,index_bytes:$index_bytes}' \
            >> "$embed_cost_rows"
    done

    policies_sha256=$(jq -cS '{reference:.policies.reference,current:.policies.current,
      fine:.policies.fine,parent:.policies.parent}' "$representation_manifest" | sha256sum | cut -d' ' -f1)
    retriever_sha256=$(jq -cS '.retriever' "$run_spec" | sha256sum | cut -d' ' -f1)
    jq -n \
        --arg accepted_sources_sha256 "$accepted_sources_sha256" \
        --arg run_spec_sha256 "$run_spec_sha256" \
        --arg queries_sha256 "$(sha_file "$queries")" \
        --arg actual_embedding_model "$actual_model" \
        --arg policies_sha256 "$policies_sha256" \
        --arg retriever_sha256 "$retriever_sha256" \
        --arg evaluator_sha256 "$evaluator_sha256" \
        --arg reference_sha256 "$(sha_file "$reference_representation")" \
        --arg current_sha256 "$(sha_file "$current_representation")" \
        --arg fine_sha256 "$(sha_file "$fine_representation")" \
        --arg child_parent_map_sha256 "$(sha_file "$child_parent_map")" \
        --arg parent_sha256 "$(sha_file "$parent_representation")" \
        --arg reference_index_sha256 "$(sha_file "$reference_db")" \
        --arg current_index_sha256 "$(sha_file "$current_db")" \
        --arg fine_index_sha256 "$(sha_file "$fine_db")" '
      {schema_version:1,accepted_sources_sha256:$accepted_sources_sha256,
       run_spec_sha256:$run_spec_sha256,
       queries_sha256:$queries_sha256,actual_embedding_model:$actual_embedding_model,
       policies_sha256:$policies_sha256,retriever_sha256:$retriever_sha256,
       evaluator_sha256:$evaluator_sha256,
       representations:{reference:$reference_sha256,current:$current_sha256,fine:$fine_sha256,
         child_parent_map:$child_parent_map_sha256,parent:$parent_sha256},
       indexes:{reference:$reference_index_sha256,current:$current_index_sha256,fine:$fine_index_sha256}}
    ' > "$run_identity.base"
    sealed_run_id=$(jq -cS . "$run_identity.base" | sha256sum | cut -d' ' -f1)
    jq --arg run_id "$sealed_run_id" '. + {run_id:$run_id}' "$run_identity.base" > "$run_identity"
    rm -f "$run_identity.base"

    jq -n \
        --argjson representation_elapsed_ms "$(elapsed_ms "$build_start" "$build_end")" \
        --slurpfile embedding "$embed_cost_rows" \
        '{representation_and_query_build:{elapsed_ms:$representation_elapsed_ms},
          embedding:$embedding,evaluation:[]}' > "$costs"
else
    for path in "$queries" "$representation_manifest" "$reference_representation" \
        "$current_representation" "$fine_representation" "$parent_representation" \
        "$child_parent_map" "$reference_db" "$current_db" "$fine_db" "$costs"; do
        if [[ ! -f "$path" ]]; then
            echo "resume artifact does not exist: $path" >&2
            exit 66
        fi
    done
    actual_model=$(jq -er '.actual_embedding_model' "$run_identity")
    policies_sha256=$(jq -cS '{reference:.policies.reference,current:.policies.current,
      fine:.policies.fine,parent:.policies.parent}' "$representation_manifest" | sha256sum | cut -d' ' -f1)
    retriever_sha256=$(jq -cS '.retriever' "$run_spec" | sha256sum | cut -d' ' -f1)
    candidate_identity=$(mktemp)
    trap 'rm -f "$candidate_identity"' EXIT
    jq -n \
        --arg accepted_sources_sha256 "$accepted_sources_sha256" \
        --arg run_spec_sha256 "$run_spec_sha256" \
        --arg queries_sha256 "$(sha_file "$queries")" \
        --arg actual_embedding_model "$actual_model" \
        --arg policies_sha256 "$policies_sha256" \
        --arg retriever_sha256 "$retriever_sha256" \
        --arg evaluator_sha256 "$evaluator_sha256" \
        --arg reference_sha256 "$(sha_file "$reference_representation")" \
        --arg current_sha256 "$(sha_file "$current_representation")" \
        --arg fine_sha256 "$(sha_file "$fine_representation")" \
        --arg child_parent_map_sha256 "$(sha_file "$child_parent_map")" \
        --arg parent_sha256 "$(sha_file "$parent_representation")" \
        --arg reference_index_sha256 "$(sha_file "$reference_db")" \
        --arg current_index_sha256 "$(sha_file "$current_db")" \
        --arg fine_index_sha256 "$(sha_file "$fine_db")" '
      {schema_version:1,accepted_sources_sha256:$accepted_sources_sha256,
       run_spec_sha256:$run_spec_sha256,
       queries_sha256:$queries_sha256,actual_embedding_model:$actual_embedding_model,
       policies_sha256:$policies_sha256,retriever_sha256:$retriever_sha256,
       evaluator_sha256:$evaluator_sha256,
       representations:{reference:$reference_sha256,current:$current_sha256,fine:$fine_sha256,
         child_parent_map:$child_parent_map_sha256,parent:$parent_sha256},
       indexes:{reference:$reference_index_sha256,current:$current_index_sha256,fine:$fine_index_sha256}}
    ' > "$candidate_identity.base"
    sealed_run_id=$(jq -cS . "$candidate_identity.base" | sha256sum | cut -d' ' -f1)
    jq --arg run_id "$sealed_run_id" '. + {run_id:$run_id}' "$candidate_identity.base" > "$candidate_identity"
    rm -f "$candidate_identity.base"
    if ! cmp -s "$candidate_identity" "$run_identity"; then
        echo "resume identity changed; refusing to continue" >&2
        exit 65
    fi
    rm -f "$candidate_identity"
    trap - EXIT
fi

actual_model=$(jq -er '.actual_embedding_model' "$run_identity")
export HKASK_EMBEDDING_MODEL=$actual_model
top_k=$(jq -r '.retriever.top_k' "$run_spec")
word_budget=$(jq -r '.retriever.word_budget' "$run_spec")
evaluation_cost_rows=$(mktemp)
trap 'rm -f "$evaluation_cost_rows"' EXIT
fidelity_failure=false
for policy in reference current fine; do
    case "$policy" in
        reference) mode=direct; representation=$reference_representation; index_db=$reference_db ;;
        current) mode=direct; representation=$current_representation; index_db=$current_db ;;
        fine) mode=small-to-big; representation=$fine_representation; index_db=$fine_db ;;
    esac
    evaluation_dir="$output_dir/evaluation-$policy"
    arguments=("$policy" "$mode" "$queries" "$representation" "$index_db" "$evaluation_dir" "$top_k" "$word_budget")
    if [[ "$mode" == small-to-big ]]; then
        arguments+=("$child_parent_map" "$parent_representation")
    fi
    started=$(now_ns)
    reused_evaluation=false
    if [[ -d "$evaluation_dir" && ! -f "$evaluation_dir/summary.json" ]]; then
        if "$evaluator" --resume "${arguments[@]}"; then
            status=0
        else
            status=$?
        fi
    elif [[ -d "$evaluation_dir" ]]; then
        summary="$evaluation_dir/summary.json"
        completion_hash="$evaluation_dir/summary.sha256"
        if [[ ! -f "$completion_hash" ]] || [[ $(cat "$completion_hash") != "$(sha_file "$summary")" ]]; then
            echo "completed evaluation summary hash changed for $policy" >&2
            exit 65
        fi
        expected_map_sha256=
        expected_parent_sha256=
        if [[ "$mode" == small-to-big ]]; then
            expected_map_sha256=$(sha_file "$child_parent_map")
            expected_parent_sha256=$(sha_file "$parent_representation")
        fi
        if ! jq -e \
            --arg policy "$policy" --arg mode "$mode" \
            --arg model "$actual_model" \
            --arg query_sha256 "$(sha_file "$queries")" \
            --arg representation_sha256 "$(sha_file "$representation")" \
            --arg index_sha256 "$(sha_file "$index_db")" \
            --arg child_parent_map_sha256 "$expected_map_sha256" \
            --arg parent_representation_sha256 "$expected_parent_sha256" \
            --argjson top_k "$top_k" --argjson word_budget "$word_budget" '
              .policy == $policy and .mode == $mode and
              .embedding_model == $model and .query_sha256 == $query_sha256 and
              .representation_sha256 == $representation_sha256 and
              .index_sha256 == $index_sha256 and
              .child_parent_map_sha256 == $child_parent_map_sha256 and
              .parent_representation_sha256 == $parent_representation_sha256 and
              .top_k == $top_k and .word_budget == $word_budget
            ' "$summary" >/dev/null; then
            echo "completed evaluation identity changed for $policy" >&2
            exit 65
        fi
        status=0
        reused_evaluation=true
    elif "$evaluator" "${arguments[@]}"; then
        status=0
    else
        status=$?
    fi
    ended=$(now_ns)
    summary="$evaluation_dir/summary.json"
    if [[ ! -f "$summary" ]]; then
        echo "evaluation failed without a summary for $policy (status=$status)" >&2
        exit "${status:-1}"
    fi
    if [[ "$reused_evaluation" != true ]]; then
        sha_file "$summary" > "$evaluation_dir/summary.sha256"
    fi
    gate=$(jq -er '.source_fidelity_gate' "$summary")
    if [[ "$gate" != pass ]]; then
        fidelity_failure=true
    elif [[ "$status" -ne 0 ]]; then
        echo "evaluation failed for $policy despite a passing fidelity summary" >&2
        exit "$status"
    fi
    if [[ "$reused_evaluation" == true ]]; then
        existing_cost=$(jq -cer --arg policy "$policy" '
          [.evaluation[] | select(.policy == $policy)]
          | if length == 1 then .[0] else error("missing or duplicate stored evaluation cost") end
        ' "$costs")
        printf '%s\n' "$existing_cost" >> "$evaluation_cost_rows"
    else
        jq -cn --arg policy "$policy" \
            --argjson elapsed_ms "$(elapsed_ms "$started" "$ended")" \
            --argjson query_count "$(jq -r '.query_count' "$summary")" \
            '{policy:$policy,elapsed_ms:$elapsed_ms,query_count:$query_count}' >> "$evaluation_cost_rows"
    fi
done

costs_tmp=$(mktemp "$output_dir/.measured-costs.tmp.XXXXXX")
jq --slurpfile evaluation "$evaluation_cost_rows" '.evaluation = $evaluation' "$costs" > "$costs_tmp"
mv "$costs_tmp" "$costs"

jq -s --slurpfile costs "$costs" --slurpfile identity "$run_identity" '
  map(select(.source_fidelity_gate == "pass")) as $eligible
  | if ($eligible | length) == 0 then error("no source-fidelity-passing policies") else
      ($eligible | sort_by([
        -(.budgeted_exact_evidence_recall),
        -(.exact_evidence_mrr),
        .duplicate_overlap.rate,
        .retrieved_words.mean,
        .index_bytes,
        .policy
      ])) as $ranked
      | {run_id:$identity[0].run_id,actual_embedding_model:$identity[0].actual_embedding_model,
         eligible_policies:[$ranked[].policy],selected_policy:$ranked[0].policy,
         ranking_rule:["budgeted_exact_evidence_recall_desc","exact_evidence_mrr_desc",
           "duplicate_overlap_rate_asc","retrieved_words_mean_asc","index_bytes_asc","policy_asc"],
         policies:$ranked,measured_costs:$costs[0]}
    end
' "$output_dir/evaluation-reference/summary.json" \
  "$output_dir/evaluation-current/summary.json" \
  "$output_dir/evaluation-fine/summary.json" > "$comparison"

printf 'run_id=%s selected_policy=%s comparison=%s\n' \
    "$(jq -r '.run_id' "$comparison")" "$(jq -r '.selected_policy' "$comparison")" "$comparison" >&2
if [[ "$fidelity_failure" == true ]]; then
    echo "one or more policies failed source fidelity; only passing policies were eligible" >&2
    exit 65
fi
