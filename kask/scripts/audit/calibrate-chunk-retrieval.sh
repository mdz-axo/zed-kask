#!/usr/bin/env bash
set -euo pipefail

# expect: A paid calibration run keeps using its own sealed execution components when the shared tree changes.
# [P1] Motivating: one reproducible derivation must retain one execution identity.
# [P2] [P3] [P4] [P8] Constraining: preserve evidence, composition, boundaries, and durable provenance.
# pre: a valid run specification and executable corpus runtime exist.
# post: fresh execution re-execs from a verified run-owned capsule; resume accepts only that capsule.

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
capsule_active=${HKASK_CALIBRATION_CAPSULE_ACTIVE:-false}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
query_builder="$repo_root/kask/scripts/audit/build-chunk-calibration-queries.sh"
host_call="$repo_root/kask/scripts/audit/call-corpus-tool-via-host.sh"
evaluator="$repo_root/kask/scripts/audit/evaluate-chunk-retrieval.sh"
inspector="$repo_root/kask/scripts/audit/inspect-chunk-calibration-run.sh"
corpus_binary=${HKASK_CORPUS_BINARY:-$HOME/.local/bin/hkask-mcp-corpus}
runtime_dir="$output_dir/runtime"
runtime_manifest="$runtime_dir/manifest.json"

for command in jq sha256sum stat date cmp cp chmod mkdir mktemp mv rmdir; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "required command not found: $command" >&2
        exit 69
    fi
done

capsule_sha_file() {
    sha256sum "$1" | cut -d' ' -f1
}
verify_runtime_capsule() {
    local component relative expected path
    if [[ ! -s "$runtime_manifest" || ! -s "$runtime_manifest.sha256" ]]; then
        echo "runtime capsule manifest is incomplete: $runtime_manifest" >&2
        return 66
    fi
    if [[ $(cat "$runtime_manifest.sha256") != "$(capsule_sha_file "$runtime_manifest")" ]]; then
        echo "runtime capsule manifest changed: $runtime_manifest" >&2
        return 65
    fi
    jq -e '
      .schema_version == 1 and
      (.components | keys | sort) == ["corpus_binary","evaluator","host_call","inspector","query_builder","runner"] and
      all(.components[];
        (.path | type == "string" and length > 0) and
        (.sha256 | type == "string" and test("^[0-9a-f]{64}$")))
    ' "$runtime_manifest" >/dev/null
    for component in runner query_builder host_call evaluator inspector corpus_binary; do
        relative=$(jq -er --arg component "$component" '.components[$component].path' "$runtime_manifest")
        expected=$(jq -er --arg component "$component" '.components[$component].sha256' "$runtime_manifest")
        path="$runtime_dir/$relative"
        if [[ ! -f "$path" || ! -x "$path" ]]; then
            echo "runtime capsule component is unavailable: $component ($path)" >&2
            return 66
        fi
        if [[ $(capsule_sha_file "$path") != "$expected" ]]; then
            echo "runtime capsule component changed: $component" >&2
            return 65
        fi
    done
}
stage_runtime_capsule() (
    local temporary source_before source_after copied component source destination
    temporary=$(mktemp -d "$output_dir/.runtime.tmp.XXXXXX")
    trap 'rm -rf "$temporary"' EXIT
    mkdir -p "$temporary/kask/scripts/audit" "$temporary/bin"
    stable_copy() {
        source=$1
        destination=$2
        component=$3
        source_before=$(capsule_sha_file "$source")
        cp "$source" "$destination"
        source_after=$(capsule_sha_file "$source")
        copied=$(capsule_sha_file "$destination")
        if [[ "$source_before" == "$source_after" && "$source_before" == "$copied" ]]; then
            :
        else
            echo "runtime source changed while staging: $component" >&2
            return 65
        fi
        chmod 0555 "$destination"
    }
    stable_copy "$repo_root/kask/scripts/audit/calibrate-chunk-retrieval.sh" \
        "$temporary/kask/scripts/audit/calibrate-chunk-retrieval.sh" runner
    stable_copy "$query_builder" "$temporary/kask/scripts/audit/build-chunk-calibration-queries.sh" query_builder
    stable_copy "$host_call" "$temporary/kask/scripts/audit/call-corpus-tool-via-host.sh" host_call
    stable_copy "$evaluator" "$temporary/kask/scripts/audit/evaluate-chunk-retrieval.sh" evaluator
    stable_copy "$inspector" "$temporary/kask/scripts/audit/inspect-chunk-calibration-run.sh" inspector
    stable_copy "$corpus_binary" "$temporary/bin/hkask-mcp-corpus" corpus_binary
    jq -cnS \
        --arg runner "$(capsule_sha_file "$temporary/kask/scripts/audit/calibrate-chunk-retrieval.sh")" \
        --arg query_builder "$(capsule_sha_file "$temporary/kask/scripts/audit/build-chunk-calibration-queries.sh")" \
        --arg host_call "$(capsule_sha_file "$temporary/kask/scripts/audit/call-corpus-tool-via-host.sh")" \
        --arg evaluator "$(capsule_sha_file "$temporary/kask/scripts/audit/evaluate-chunk-retrieval.sh")" \
        --arg inspector "$(capsule_sha_file "$temporary/kask/scripts/audit/inspect-chunk-calibration-run.sh")" \
        --arg corpus_binary "$(capsule_sha_file "$temporary/bin/hkask-mcp-corpus")" \
        '{schema_version:1,components:{
          runner:{path:"kask/scripts/audit/calibrate-chunk-retrieval.sh",sha256:$runner},
          query_builder:{path:"kask/scripts/audit/build-chunk-calibration-queries.sh",sha256:$query_builder},
          host_call:{path:"kask/scripts/audit/call-corpus-tool-via-host.sh",sha256:$host_call},
          evaluator:{path:"kask/scripts/audit/evaluate-chunk-retrieval.sh",sha256:$evaluator},
          inspector:{path:"kask/scripts/audit/inspect-chunk-calibration-run.sh",sha256:$inspector},
          corpus_binary:{path:"bin/hkask-mcp-corpus",sha256:$corpus_binary}}}' > "$temporary/manifest.json"
    capsule_sha_file "$temporary/manifest.json" > "$temporary/manifest.json.sha256"
    chmod 0444 "$temporary/manifest.json" "$temporary/manifest.json.sha256"
    chmod 0755 "$temporary/kask/scripts/audit" "$temporary/kask/scripts" "$temporary/kask" "$temporary/bin"
    mv "$temporary" "$runtime_dir"
    trap - EXIT
)
exec_runtime_capsule() {
    local runner="$runtime_dir/kask/scripts/audit/calibrate-chunk-retrieval.sh"
    local binary="$runtime_dir/bin/hkask-mcp-corpus"
    if [[ "$resume" == true ]]; then
        exec env HKASK_CALIBRATION_CAPSULE_ACTIVE=true HKASK_CORPUS_BINARY="$binary" \
            bash "$runner" --resume "$run_spec" "$output_dir"
    fi
    exec env HKASK_CALIBRATION_CAPSULE_ACTIVE=true HKASK_CORPUS_BINARY="$binary" \
        bash "$runner" "$run_spec" "$output_dir"
}

if [[ ! -f "$run_spec" ]]; then
    echo "run spec does not exist: $run_spec" >&2
    exit 66
fi
if [[ "$resume" == true && "$capsule_active" != true ]]; then
    if [[ ! -d "$output_dir" || ! -f "$output_dir/run-preseal-identity.json" ]]; then
        echo "resume requires an output directory with run-preseal-identity.json" >&2
        exit 66
    fi
    verify_runtime_capsule
    exec_runtime_capsule
fi
for script in "$query_builder" "$host_call" "$evaluator" "$inspector"; do
    if [[ ! -x "$script" ]]; then
        echo "required executable script does not exist: $script" >&2
        exit 66
    fi
done
if [[ ! -f "$corpus_binary" ]]; then
    echo "corpus binary does not exist: $corpus_binary" >&2
    exit 66
fi
jq -e '
  (.accepted_sources | type == "array" and length > 0) and
  (.entity_ref_prefix | type == "string" and length > 0) and
  (.embedding_model | type == "string" and length > 0) and
  (.batch_size | type == "number" and . > 0 and floor == .) and
  (.embedding_max_concurrency | type == "number" and . > 0 and floor == .) and
  (.embedding_shard_retry_limit | type == "number" and . > 0 and floor == .) and
  (.embedding_retry_backoff_secs | type == "number" and . >= 0 and floor == .) and
  (.max_queries | type == "number" and . > 0 and floor == .) and
  (.retriever.name == "corpus_query_cosine") and
  (.retriever.top_k | type == "number" and . > 0 and . <= 50 and floor == .) and
  (.retriever.word_budget | type == "number" and . > 0 and floor == .) and
  (.retriever.min_score == 0) and
  (.selection.max_budgeted_exact_evidence_loss_count | type == "number" and . >= 0 and floor == .) and
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
    if [[ ! -d "$output_dir" || ! -f "$output_dir/run-preseal-identity.json" ]]; then
        echo "resume requires an output directory with run-preseal-identity.json" >&2
        exit 66
    fi
elif [[ "$capsule_active" == true ]]; then
    if [[ ! -d "$output_dir" ]]; then
        echo "active runtime capsule output directory does not exist: $output_dir" >&2
        exit 66
    fi
else
    if [[ -e "$output_dir" ]]; then
        echo "refusing to overwrite output directory: $output_dir" >&2
        exit 73
    fi
    mkdir -p "$output_dir"
    if ! stage_runtime_capsule; then
        rmdir "$output_dir" 2>/dev/null || true
        echo "failed to stage runtime capsule" >&2
        exit 65
    fi
    verify_runtime_capsule
    exec_runtime_capsule
fi
if [[ "$capsule_active" == true ]]; then
    expected_repo_root=$(cd "$runtime_dir" && pwd)
    if [[ "$repo_root" != "$expected_repo_root" ]]; then
        echo "active runtime capsule does not own the runner path" >&2
        exit 65
    fi
    verify_runtime_capsule
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
preseal_identity="$output_dir/run-preseal-identity.json"
model_identity="$output_dir/embedding-model-identity.json"
run_identity="$output_dir/run-identity.json"
representation_cost="$output_dir/representation-build-cost.json"
costs="$output_dir/measured-costs.json"
comparison="$output_dir/comparison.json"
checkpoint_dir="$output_dir/embed/checkpoints"
embed_shard_max_bytes=${HKASK_CALIBRATION_EMBED_SHARD_MAX_BYTES:-8000000}
if [[ ! "$embed_shard_max_bytes" =~ ^[1-9][0-9]*$ ]]; then
    echo "HKASK_CALIBRATION_EMBED_SHARD_MAX_BYTES must be a positive integer" >&2
    exit 64
fi

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
split_jsonl_by_bytes() {
    local input=$1 destination=$2 max_bytes=$3 reassembled
    mkdir -p "$destination"
    LC_ALL=C awk -v destination="$destination" -v max_bytes="$max_bytes" '
      BEGIN { shard = 0; shard_bytes = 0 }
      {
        line_bytes = length($0) + 1
        if (line_bytes > max_bytes) {
          printf "JSONL record exceeds embedding shard limit: %d > %d bytes\n", line_bytes, max_bytes > "/dev/stderr"
          exit 65
        }
        if (shard_bytes > 0 && shard_bytes + line_bytes > max_bytes) {
          shard++
          shard_bytes = 0
        }
        path = sprintf("%s/shard-%05d.jsonl", destination, shard)
        print $0 >> path
        shard_bytes += line_bytes
      }
      END { if (NR == 0) exit 66 }
    ' "$input"
    reassembled=$(mktemp "$destination/.reassembled.XXXXXX")
    cat "$destination"/shard-[0-9][0-9][0-9][0-9][0-9].jsonl > "$reassembled"
    if ! cmp "$input" "$reassembled"; then
        rm -f "$reassembled"
        echo "embedding shards do not reassemble to the source representation: $input" >&2
        return 65
    fi
    rm -f "$reassembled"
}
verify_shards_reassemble() {
    local input=$1 destination=$2 reassembled
    if ! compgen -G "$destination/shard-[0-9][0-9][0-9][0-9][0-9].jsonl" >/dev/null; then
        echo "embedding shard set is missing: $destination" >&2
        return 66
    fi
    reassembled=$(mktemp "$destination/.reassembled.XXXXXX")
    cat "$destination"/shard-[0-9][0-9][0-9][0-9][0-9].jsonl > "$reassembled"
    if ! cmp "$input" "$reassembled"; then
        rm -f "$reassembled"
        echo "embedding shards do not reassemble to the source representation: $input" >&2
        return 65
    fi
    rm -f "$reassembled"
}
shard_inventory() {
    local destination=$1 shard
    for shard in "$destination"/shard-[0-9][0-9][0-9][0-9][0-9].jsonl; do
        jq -cn --arg name "$(basename "$shard" .jsonl)" \
            --arg sha256 "$(sha_file "$shard")" \
            --argjson rows "$(wc -l < "$shard" | tr -d ' ')" \
            '{name:$name,sha256:$sha256,rows:$rows}'
    done | jq -sc .
}
publish_hashed_json() {
    local source=$1 destination=$2 temporary checksum_temporary
    temporary=$(mktemp "${destination}.tmp.XXXXXX")
    checksum_temporary=$(mktemp "${destination}.sha256.tmp.XXXXXX")
    jq -cS . "$source" > "$temporary"
    sha_file "$temporary" > "$checksum_temporary"
    mv "$temporary" "$destination"
    mv "$checksum_temporary" "$destination.sha256"
}
verify_hashed_json() {
    local path=$1 expected
    if [[ ! -s "$path" || ! -s "$path.sha256" ]]; then
        echo "hashed JSON artifact is incomplete: $path" >&2
        return 66
    fi
    expected=$(cat "$path.sha256")
    if [[ "$expected" != "$(sha_file "$path")" ]]; then
        echo "hashed JSON artifact changed: $path" >&2
        return 65
    fi
    jq -e 'type == "object"' "$path" >/dev/null
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
requested_model=$(jq -r '.embedding_model' "$run_spec")

batch_size=$(jq -r '.batch_size' "$run_spec")
embedding_max_concurrency=$(jq -r '.embedding_max_concurrency' "$run_spec")
embedding_shard_retry_limit=$(jq -r '.embedding_shard_retry_limit' "$run_spec")
embedding_retry_backoff_secs=$(jq -r '.embedding_retry_backoff_secs' "$run_spec")
export HKASK_EMBEDDING_MODEL=$requested_model
export HKASK_MAX_CONCURRENCY=$embedding_max_concurrency

build_preseal_candidate() {
    local destination=$1 reference_shards current_shards fine_shards base run_id
    reference_shards=$(shard_inventory "$output_dir/embed/reference-shards")
    current_shards=$(shard_inventory "$output_dir/embed/current-shards")
    fine_shards=$(shard_inventory "$output_dir/embed/fine-shards")
    base=$(mktemp "$output_dir/.preseal-base.XXXXXX")
    jq -n \
        --arg accepted_sources_sha256 "$accepted_sources_sha256" \
        --arg run_spec_sha256 "$run_spec_sha256" \
        --arg queries_sha256 "$(sha_file "$queries")" \
        --arg requested_embedding_model "$requested_model" \
        --arg policies_sha256 "$(jq -cS '{reference:.policies.reference,current:.policies.current,fine:.policies.fine,parent:.policies.parent}' "$representation_manifest" | sha256sum | cut -d' ' -f1)" \
        --arg retriever_sha256 "$(jq -cS '.retriever' "$run_spec" | sha256sum | cut -d' ' -f1)" \
        --arg evaluator_sha256 "$evaluator_sha256" \
        --arg runner_sha256 "$(sha_file "$repo_root/kask/scripts/audit/calibrate-chunk-retrieval.sh")" \
        --arg query_builder_sha256 "$(sha_file "$query_builder")" \
        --arg inspector_sha256 "$(sha_file "$inspector")" \
        --arg host_call_sha256 "$(sha_file "$host_call")" \
        --arg corpus_binary_sha256 "$(sha_file "$corpus_binary")" \
        --arg runtime_manifest_sha256 "$(sha_file "$runtime_manifest")" \
        --arg representation_cost_sha256 "$(sha_file "$representation_cost")" \
        --arg representations_manifest_sha256 "$(sha_file "$representation_manifest")" \
        --arg reference_sha256 "$(sha_file "$reference_representation")" \
        --arg current_sha256 "$(sha_file "$current_representation")" \
        --arg fine_sha256 "$(sha_file "$fine_representation")" \
        --arg child_parent_map_sha256 "$(sha_file "$child_parent_map")" \
        --arg parent_sha256 "$(sha_file "$parent_representation")" \
        --argjson batch_size "$batch_size" \
        --argjson max_concurrency "$embedding_max_concurrency" \
        --argjson retry_limit "$embedding_shard_retry_limit" \
        --argjson retry_backoff_secs "$embedding_retry_backoff_secs" \
        --argjson shard_max_bytes "$embed_shard_max_bytes" \
        --argjson reference_shards "$reference_shards" \
        --argjson current_shards "$current_shards" \
        --argjson fine_shards "$fine_shards" '
      {schema_version:3,accepted_sources_sha256:$accepted_sources_sha256,
       representations_manifest_sha256:$representations_manifest_sha256,
       run_spec_sha256:$run_spec_sha256,queries_sha256:$queries_sha256,
       requested_embedding_model:$requested_embedding_model,
       policies_sha256:$policies_sha256,retriever_sha256:$retriever_sha256,
       evaluator_sha256:$evaluator_sha256,runner_sha256:$runner_sha256,
       query_builder_sha256:$query_builder_sha256,inspector_sha256:$inspector_sha256,
       host_call_sha256:$host_call_sha256,
       corpus_binary_sha256:$corpus_binary_sha256,runtime_manifest_sha256:$runtime_manifest_sha256,
       representation_cost_sha256:$representation_cost_sha256,
       embedding:{batch_size:$batch_size,max_concurrency:$max_concurrency,
         retry_limit:$retry_limit,retry_backoff_secs:$retry_backoff_secs,
         shard_max_bytes:$shard_max_bytes},
       representations:{reference:$reference_sha256,current:$current_sha256,fine:$fine_sha256,
         child_parent_map:$child_parent_map_sha256,parent:$parent_sha256},
       shards:{reference:$reference_shards,current:$current_shards,fine:$fine_shards}}
    ' > "$base"
    run_id=$(jq -cS . "$base" | sha256sum | cut -d' ' -f1)
    jq -cS --arg run_id "$run_id" '. + {run_id:$run_id}' "$base" > "$destination"
    rm -f "$base"
}

if [[ "$resume" != true ]]; then
    max_queries=$(jq -r '.max_queries' "$run_spec")
    build_start=$(now_ns)
    "$query_builder" "$run_spec" "$queries" "$max_queries"
    jq -n --slurpfile spec "$run_spec" --arg output_dir "$representations_dir" '
      {accepted_sources:$spec[0].accepted_sources,output_dir:$output_dir,
       entity_ref_prefix:$spec[0].entity_ref_prefix,current_policy:$spec[0].policies.current,
       fine_policy:$spec[0].policies.fine,parent_policy:$spec[0].policies.parent}
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
    representation_cost_source=$(mktemp "$output_dir/.representation-cost.XXXXXX")
    jq -n --argjson elapsed_ms "$(elapsed_ms "$build_start" "$build_end")" \
        '{elapsed_ms:$elapsed_ms}' > "$representation_cost_source"
    publish_hashed_json "$representation_cost_source" "$representation_cost"
    rm -f "$representation_cost_source"

    mkdir -p "$output_dir/embed" "$checkpoint_dir"
    split_jsonl_by_bytes "$reference_representation" "$output_dir/embed/reference-shards" "$embed_shard_max_bytes"
    split_jsonl_by_bytes "$current_representation" "$output_dir/embed/current-shards" "$embed_shard_max_bytes"
    split_jsonl_by_bytes "$fine_representation" "$output_dir/embed/fine-shards" "$embed_shard_max_bytes"
    preseal_candidate=$(mktemp "$output_dir/.preseal.XXXXXX")
    build_preseal_candidate "$preseal_candidate"
    publish_hashed_json "$preseal_candidate" "$preseal_identity"
    rm -f "$preseal_candidate"
else
    for path in "$queries" "$representation_manifest" "$reference_representation" \
        "$current_representation" "$fine_representation" "$parent_representation" \
        "$child_parent_map" "$representation_cost"; do
        if [[ ! -f "$path" ]]; then
            echo "resume artifact does not exist: $path" >&2
            exit 66
        fi
    done
    verify_hashed_json "$representation_cost"
    verify_shards_reassemble "$reference_representation" "$output_dir/embed/reference-shards"
    verify_shards_reassemble "$current_representation" "$output_dir/embed/current-shards"
    verify_shards_reassemble "$fine_representation" "$output_dir/embed/fine-shards"
    verify_hashed_json "$preseal_identity"
    preseal_candidate=$(mktemp "$output_dir/.preseal.XXXXXX")
    build_preseal_candidate "$preseal_candidate"
    if ! cmp -s "$preseal_candidate" "$preseal_identity"; then
        rm -f "$preseal_candidate"
        echo "resume preseal identity changed; refusing to continue" >&2
        exit 65
    fi
    rm -f "$preseal_candidate"
fi

jq -e '
  (.schema_version == 2) and
  (.boilerplate_exclusion_reports | type == "object") and
  ((.boilerplate_exclusion_reports | length) == .validation.accepted_source_count) and
  all(.boilerplate_exclusion_reports[];
    (.input_words | type == "number") and
    (.retained_words | type == "number") and
    (.retained_words <= .input_words) and
    (.exclusions | type == "array")) and
  .validation.boilerplate_filter_applied and
  .validation.unique_entity_refs and .validation.normalized_source_reconstruction and
  .validation.every_child_mapped and .validation.every_parent_exists and
  .validation.child_map_parent_sources_agree
' "$representation_manifest" >/dev/null

verify_no_retained_boilerplate() {
    local representation
    for representation in \
        "$reference_representation" "$current_representation" \
        "$fine_representation" "$parent_representation"; do
        if jq -e '
          def retained_boilerplate:
            (.text | contains("OceanofPDF.com")) or
            (.text | contains("Thanks for reading ")) or
            (.text | contains("Subscribe for free to receive new posts and support my work.")) or
            (.text | contains("Share Merchant Adventures")) or
            (.text | contains("Subscribe now")) or
            (.text | contains("Leave a comment")) or
            (.text | contains("Play in Reduct")) or
            (.text | contains("This page intentionally left blank")) or
            (([.text | scan("\\\\qquad")] | length) as $occurrences
              | $occurrences >= 8 and
                ($occurrences * 2 >= ([.text | scan("\\S+")] | length)));
          select(retained_boilerplate)
        ' "$representation" >/dev/null; then
            echo "representation retains forbidden watermark or boilerplate: $representation" >&2
            return 65
        fi
    done
}
verify_no_retained_boilerplate

verify_hashed_json "$preseal_identity"
preseal_run_id=$(jq -er '.run_id' "$preseal_identity")
verify_runtime_identity() {
    local label path field expected actual
    while [[ $# -gt 0 ]]; do
        label=$1
        path=$2
        field=$3
        shift 3
        expected=$(jq -er ".$field" "$preseal_identity")
        actual=$(sha_file "$path")
        if [[ "$actual" != "$expected" ]]; then
            echo "runtime identity changed for $label: sealed=$expected actual=$actual" >&2
            return 65
        fi
    done
}
verify_runtime_executables() {
    verify_runtime_capsule
    verify_runtime_identity \
        runner "$repo_root/kask/scripts/audit/calibrate-chunk-retrieval.sh" runner_sha256 \
        query_builder "$query_builder" query_builder_sha256 \
        host_call "$host_call" host_call_sha256 \
        evaluator "$evaluator" evaluator_sha256 \
        inspector "$inspector" inspector_sha256 \
        corpus_binary "$corpus_binary" corpus_binary_sha256 \
        runtime_manifest "$runtime_manifest" runtime_manifest_sha256
}
verify_runtime_executables
mkdir -p "$checkpoint_dir"
actual_model=
if [[ -e "$model_identity" || -e "$model_identity.sha256" ]]; then
    verify_hashed_json "$model_identity"
    if ! jq -e --arg preseal "$preseal_run_id" --arg requested "$requested_model" '
      .preseal_run_id == $preseal and .requested_model == $requested and
      (.actual_model | type == "string" and length > 0)
    ' "$model_identity" >/dev/null; then
        echo "embedding model identity does not match preseal identity" >&2
        exit 65
    fi
    actual_model=$(jq -r '.actual_model' "$model_identity")
fi

inventory_shard() {
    local policy=$1 shard=$2 index_db=$3 expected_model=$4 tag=$5
    local shard_name arguments response log content
    tag="$tag-$(now_ns)"
    shard_name=$(basename "$shard" .jsonl)
    arguments="$output_dir/embed/inventory-$policy-$shard_name-$tag-arguments.json"
    response="$output_dir/embed/inventory-$policy-$shard_name-$tag-response.json"
    log="$output_dir/embed/inventory-$policy-$shard_name-$tag-server.log"
    jq -n --arg chunks_jsonl "$shard" --arg db_path "$index_db" \
        --arg expected_model "$expected_model" \
        '{chunks_jsonl:$chunks_jsonl,db_path:$db_path,expected_model:$expected_model}' > "$arguments"
    "$host_call" corpus_embedding_inventory "$arguments" "$response" "$log"
    content=$(tool_content "$response")
    if ! jq -e --arg model "$expected_model" '.expected_model == $model' <<<"$content" >/dev/null; then
        echo "embedding inventory changed expected model for $policy/$shard_name" >&2
        return 65
    fi
    printf '%s\n' "$content"
}

embed_cost_rows="$output_dir/embed-costs.jsonl"
: > "$embed_cost_rows"
for policy in reference current fine; do
    case "$policy" in
        reference) representation=$reference_representation; index_db=$reference_db ;;
        current) representation=$current_representation; index_db=$current_db ;;
        fine) representation=$fine_representation; index_db=$fine_db ;;
    esac
    shard_dir="$output_dir/embed/$policy-shards"
    verify_shards_reassemble "$representation" "$shard_dir"
    expected_total=$(wc -l < "$representation" | tr -d ' ')
    for shard in "$shard_dir"/shard-[0-9][0-9][0-9][0-9][0-9].jsonl; do
        verify_runtime_executables
        shard_name=$(basename "$shard" .jsonl)
        shard_sha256=$(sha_file "$shard")
        expected_shard_sha256=$(jq -er --arg policy "$policy" --arg shard "$shard_name" \
            '.shards[$policy][] | select(.name == $shard) | .sha256' "$preseal_identity")
        if [[ "$shard_sha256" != "$expected_shard_sha256" ]]; then
            echo "runtime shard identity changed for $policy/$shard_name" >&2
            exit 65
        fi
        shard_expected=$(wc -l < "$shard" | tr -d ' ')
        checkpoint="$checkpoint_dir/$policy-$shard_name.json"
        if [[ -e "$checkpoint" || -e "$checkpoint.sha256" ]]; then
            verify_hashed_json "$checkpoint"
            if [[ -z "$actual_model" ]]; then
                echo "completed shard checkpoint exists without embedding model identity" >&2
                exit 65
            fi
            if ! jq -e --arg preseal "$preseal_run_id" --arg policy "$policy" \
                --arg shard "$shard_name" --arg shard_sha256 "$shard_sha256" \
                --arg requested "$requested_model" --arg actual "$actual_model" \
                --argjson rows "$shard_expected" '
              .preseal_run_id == $preseal and .policy == $policy and .shard == $shard and
              .shard_sha256 == $shard_sha256 and .requested_model == $requested and
              .actual_model == $actual and .embedded_rows == $rows and .complete == true
            ' "$checkpoint" >/dev/null; then
                echo "completed shard checkpoint identity changed: $policy/$shard_name" >&2
                exit 65
            fi
            inventory=$(inventory_shard "$policy" "$shard" "$index_db" "$actual_model" "checkpoint-$$")
            if ! jq -e --argjson rows "$shard_expected" '
              .complete == true and .requested == $rows and .stored_matching_model == $rows and
              (.missing_entity_refs | length) == 0 and (.mismatched_model_entity_refs | length) == 0
            ' <<<"$inventory" >/dev/null; then
                echo "completed shard checkpoint does not reconcile with durable storage: $policy/$shard_name" >&2
                exit 65
            fi
            continue
        fi

        recovered_rows=0
        pending=$shard
        if [[ "$resume" == true && -f "$index_db" ]]; then
            if [[ -z "$actual_model" ]]; then
                echo "partial embedding database cannot be reconciled before one provider-confirmed model identity is durable" >&2
                exit 65
            fi
            inventory=$(inventory_shard "$policy" "$shard" "$index_db" "$actual_model" "recovery-$$")
            recovered_rows=$(jq -er '.stored_matching_model' <<<"$inventory")
            retry_refs=$(mktemp "$output_dir/embed/.retry-refs.XXXXXX")
            jq '.retry_entity_refs' <<<"$inventory" > "$retry_refs"
            pending="$shard_dir/$shard_name-recovery-$$.jsonl"
            jq -c --slurpfile retry "$retry_refs" \
                'select(.entity_ref as $ref | ($retry[0] | index($ref)) != null)' "$shard" > "$pending"
            rm -f "$retry_refs"
            if [[ $(wc -l < "$pending" | tr -d ' ') -ne $(jq -r '.retry_entity_refs | length' <<<"$inventory") ]]; then
                echo "durable inventory retry selection did not reconcile for $policy/$shard_name" >&2
                exit 65
            fi
        fi

        shard_embedded=$recovered_rows
        shard_attempted=0
        shard_elapsed_ms=0
        shard_attempts=0
        for prior_arguments in "$output_dir/embed/$policy-$shard_name-attempt-"[0-9][0-9]-arguments.json; do
            [[ -f "$prior_arguments" ]] || continue
            prior_input=$(jq -er '.chunks_jsonl | select(type == "string" and length > 0)' "$prior_arguments")
            if [[ ! -f "$prior_input" ]]; then
                echo "prior embedding attempt input is missing: $prior_input" >&2
                exit 65
            fi
            shard_attempted=$((shard_attempted + $(wc -l < "$prior_input" | tr -d ' ')))
            shard_attempts=$((shard_attempts + 1))
        done
        prior_attempts=$shard_attempts
        attempt=1
        while compgen -G "$output_dir/embed/$policy-$shard_name-attempt-$(printf '%02d' "$attempt")-*" >/dev/null; do
            attempt=$((attempt + 1))
        done
        if [[ $((attempt - 1)) -ne "$prior_attempts" ]]; then
            echo "embedding attempt artifacts are not contiguous for $policy/$shard_name" >&2
            exit 65
        fi
        while [[ "$shard_embedded" -lt "$shard_expected" ]]; do
            if [[ "$attempt" -gt "$embedding_shard_retry_limit" ]]; then
                echo "embedding shard exhausted retry limit for $policy/$shard_name: attempts=$shard_attempts" >&2
                exit 65
            fi
            attempt_label=$(printf '%02d' "$attempt")
            arguments="$output_dir/embed/$policy-$shard_name-attempt-$attempt_label-arguments.json"
            response="$output_dir/embed/$policy-$shard_name-attempt-$attempt_label-response.json"
            log="$output_dir/embed/$policy-$shard_name-attempt-$attempt_label-server.log"
            jq -n --arg chunks_jsonl "$pending" --arg db_path "$index_db" \
                --arg model "$requested_model" --argjson batch_size "$batch_size" \
                '{chunks_jsonl:$chunks_jsonl,tagged_jsonl:null,db_path:$db_path,model:$model,batch_size:$batch_size}' > "$arguments"
            started=$(now_ns)
            verify_runtime_executables
            "$host_call" corpus_embed "$arguments" "$response" "$log"
            ended=$(now_ns)
            content=$(tool_content "$response")
            reported_requested_model=$(jq -er '.requested_model | select(type == "string" and length > 0)' <<<"$content")
            model_status=$(jq -er '.actual_model_status | select(type == "string")' <<<"$content")
            model=$(jq -er '.actual_model | select(type == "string" and length > 0)' <<<"$content")
            embedded=$(jq -er '.embedded' <<<"$content")
            failed=$(jq -er '.failed' <<<"$content")
            cancelled=$(jq -r '.cancelled' <<<"$content")
            total=$(jq -er '.total' <<<"$content")
            if [[ "$reported_requested_model" != "$requested_model" || "$model_status" != confirmed ]]; then
                echo "embedding response did not preserve confirmed requested/actual model identity for $policy/$shard_name" >&2
                exit 65
            fi
            if [[ "$cancelled" != 0 && "$cancelled" != false || $((embedded + failed)) -ne "$total" ]]; then
                echo "embedding accounting failed for $policy/$shard_name attempt $attempt" >&2
                exit 65
            fi
            if [[ -z "$actual_model" ]]; then
                model_source=$(mktemp "$output_dir/embed/.model-identity.XXXXXX")
                jq -n --arg preseal "$preseal_run_id" --arg requested "$requested_model" \
                    --arg actual "$model" \
                    '{schema_version:1,preseal_run_id:$preseal,requested_model:$requested,actual_model:$actual}' > "$model_source"
                publish_hashed_json "$model_source" "$model_identity"
                rm -f "$model_source"
                actual_model=$model
            elif [[ "$actual_model" != "$model" ]]; then
                echo "embedding shards used different provider-confirmed actual models: $actual_model vs $model" >&2
                exit 65
            fi
            shard_embedded=$((shard_embedded + embedded))
            shard_attempted=$((shard_attempted + total))
            shard_elapsed_ms=$((shard_elapsed_ms + $(elapsed_ms "$started" "$ended")))
            shard_attempts=$((shard_attempts + 1))
            if [[ "$failed" -gt 0 ]]; then
                refs_complete=$(jq -r '.failed_entity_refs_complete' <<<"$content")
                refs_count=$(jq -r '.failed_entity_refs | length' <<<"$content")
                if [[ "$refs_complete" != true || "$refs_count" -ne "$failed" ]]; then
                    echo "embedding shard did not return a complete failed-ref set for $policy/$shard_name" >&2
                    exit 65
                fi
                failed_refs="$output_dir/embed/$policy-$shard_name-attempt-$attempt_label-failed-refs.json"
                retry_file="$shard_dir/$shard_name-retry-$attempt_label.jsonl"
                jq '.failed_entity_refs' <<<"$content" > "$failed_refs"
                jq -c --slurpfile failed "$failed_refs" \
                    'select(.entity_ref as $ref | ($failed[0] | index($ref)) != null)' "$pending" > "$retry_file"
                if [[ $(wc -l < "$retry_file" | tr -d ' ') -ne "$failed" ]]; then
                    echo "failed-ref retry selection did not reconcile for $policy/$shard_name" >&2
                    exit 65
                fi
                sleep_seconds=$((embedding_retry_backoff_secs * (shard_attempts)))
                if [[ "$sleep_seconds" -gt 0 ]]; then sleep "$sleep_seconds"; fi
                pending=$retry_file
            fi
            attempt=$((attempt + 1))
        done
        if [[ "$shard_embedded" -ne "$shard_expected" ]]; then
            echo "embedding totals do not reconcile for $policy/$shard_name" >&2
            exit 65
        fi
        checkpoint_source=$(mktemp "$checkpoint_dir/.checkpoint.XXXXXX")
        jq -n --arg preseal "$preseal_run_id" --arg policy "$policy" --arg shard "$shard_name" \
            --arg shard_sha256 "$shard_sha256" --arg requested "$requested_model" \
            --arg actual "$actual_model" --argjson rows "$shard_expected" \
            --argjson attempted "$shard_attempted" --argjson recovered "$recovered_rows" \
            --argjson elapsed_ms "$shard_elapsed_ms" --argjson attempts "$shard_attempts" \
            --argjson prior_attempts "$prior_attempts" '
          {schema_version:1,preseal_run_id:$preseal,policy:$policy,shard:$shard,
           shard_sha256:$shard_sha256,requested_model:$requested,actual_model:$actual,
           embedded_rows:$rows,attempted_rows:$attempted,recovered_rows:$recovered,
           elapsed_ms:$elapsed_ms,attempts:$attempts,
           unmeasured_interrupted_attempts:$prior_attempts,complete:true}
        ' > "$checkpoint_source"
        publish_hashed_json "$checkpoint_source" "$checkpoint"
        rm -f "$checkpoint_source"
    done

    if [[ ! -f "$index_db" ]]; then
        echo "corpus_embed did not create index: $index_db" >&2
        exit 65
    fi
    checkpoint_glob=("$checkpoint_dir/$policy-"shard-*.json)
    jq -s --arg policy "$policy" --arg model "$actual_model" \
        --argjson total "$expected_total" --argjson shard_max_bytes "$embed_shard_max_bytes" \
        --argjson max_concurrency "$embedding_max_concurrency" \
        --argjson index_bytes "$(stat -c %s "$index_db")" '
      {policy:$policy,actual_embedding_model:$model,
       elapsed_ms:(map(.elapsed_ms)|add),total_rows:$total,
       attempted_rows:(map(.attempted_rows)|add),embedded_rows:(map(.embedded_rows)|add),
       recovered_rows:(map(.recovered_rows)|add),shards:length,attempts:(map(.attempts)|add),
       unmeasured_interrupted_attempts:(map(.unmeasured_interrupted_attempts)|add),
       shard_max_bytes:$shard_max_bytes,max_concurrency:$max_concurrency,index_bytes:$index_bytes}
    ' "${checkpoint_glob[@]}" >> "$embed_cost_rows"
done

if [[ -z "$actual_model" ]]; then
    echo "embedding completed without a provider-confirmed actual model identity" >&2
    exit 65
fi
policies_sha256=$(jq -cS '{reference:.policies.reference,current:.policies.current,fine:.policies.fine,parent:.policies.parent}' "$representation_manifest" | sha256sum | cut -d' ' -f1)
retriever_sha256=$(jq -cS '.retriever' "$run_spec" | sha256sum | cut -d' ' -f1)
identity_candidate=$(mktemp "$output_dir/.run-identity.XXXXXX")
jq -n --arg preseal_run_id "$preseal_run_id" \
    --arg accepted_sources_sha256 "$accepted_sources_sha256" --arg run_spec_sha256 "$run_spec_sha256" \
    --arg queries_sha256 "$(sha_file "$queries")" --arg requested_embedding_model "$requested_model" \
    --arg actual_embedding_model "$actual_model" --arg policies_sha256 "$policies_sha256" \
    --arg retriever_sha256 "$retriever_sha256" --arg evaluator_sha256 "$evaluator_sha256" \
    --arg representations_manifest_sha256 "$(sha_file "$representation_manifest")" \
    --arg reference_sha256 "$(sha_file "$reference_representation")" \
    --arg current_sha256 "$(sha_file "$current_representation")" --arg fine_sha256 "$(sha_file "$fine_representation")" \
    --arg child_parent_map_sha256 "$(sha_file "$child_parent_map")" --arg parent_sha256 "$(sha_file "$parent_representation")" \
    --arg reference_index_sha256 "$(sha_file "$reference_db")" --arg current_index_sha256 "$(sha_file "$current_db")" \
    --arg fine_index_sha256 "$(sha_file "$fine_db")" '
  {schema_version:3,preseal_run_id:$preseal_run_id,accepted_sources_sha256:$accepted_sources_sha256,
   representations_manifest_sha256:$representations_manifest_sha256,
   run_spec_sha256:$run_spec_sha256,queries_sha256:$queries_sha256,
   requested_embedding_model:$requested_embedding_model,actual_embedding_model:$actual_embedding_model,
   policies_sha256:$policies_sha256,retriever_sha256:$retriever_sha256,evaluator_sha256:$evaluator_sha256,
   representations:{reference:$reference_sha256,current:$current_sha256,fine:$fine_sha256,
     child_parent_map:$child_parent_map_sha256,parent:$parent_sha256},
   indexes:{reference:$reference_index_sha256,current:$current_index_sha256,fine:$fine_index_sha256}}
' > "$identity_candidate.base"
sealed_run_id=$(jq -cS . "$identity_candidate.base" | sha256sum | cut -d' ' -f1)
jq --arg run_id "$sealed_run_id" '. + {run_id:$run_id}' "$identity_candidate.base" > "$identity_candidate"
rm -f "$identity_candidate.base"
if [[ -f "$run_identity" ]]; then
    if ! cmp -s "$identity_candidate" "$run_identity"; then
        rm -f "$identity_candidate"
        echo "resume final identity changed; refusing to continue" >&2
        exit 65
    fi
    rm -f "$identity_candidate"
else
    mv "$identity_candidate" "$run_identity"
fi

costs_tmp=$(mktemp "$output_dir/.measured-costs.tmp.XXXXXX")
jq -n --slurpfile representation "$representation_cost" --slurpfile embedding "$embed_cost_rows" \
    '{representation_and_query_build:$representation[0],embedding:$embedding,evaluation:[]}' > "$costs_tmp"
if [[ -f "$costs" ]]; then
    jq --slurpfile rebuilt "$costs_tmp" '.representation_and_query_build=$rebuilt[0].representation_and_query_build | .embedding=$rebuilt[0].embedding' "$costs" > "$costs_tmp.merged"
    mv "$costs_tmp.merged" "$costs_tmp"
fi
mv "$costs_tmp" "$costs"
export HKASK_EMBEDDING_MODEL=$requested_model
top_k=$(jq -r '.retriever.top_k' "$run_spec")
word_budget=$(jq -r '.retriever.word_budget' "$run_spec")
max_budgeted_loss_count=$(jq -r '.selection.max_budgeted_exact_evidence_loss_count' "$run_spec")
evaluation_cost_rows=$(mktemp)
trap 'rm -f "$evaluation_cost_rows"' EXIT
fidelity_failure=false
for policy in reference current fine; do
    verify_runtime_executables
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
            --arg model "$requested_model" \
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

jq -s --slurpfile costs "$costs" --slurpfile identity "$run_identity" \
  --argjson max_budgeted_loss_count "$max_budgeted_loss_count" '
  map(select(.source_fidelity_gate == "pass")) as $eligible
  | if ($eligible | length) == 0 then error("no source-fidelity-passing policies") else
      ([$eligible[] | (.budgeted_exact_evidence_recall * .query_count | round)] | max) as $best_hit_count
      | [$eligible[] | select(($best_hit_count - (.budgeted_exact_evidence_recall * .query_count | round)) <= $max_budgeted_loss_count)] as $material_contenders
      | ($material_contenders | sort_by([
          -(.budgeted_correct_source_recall),
          .duplicate_overlap.rate,
          -(.exact_evidence_mrr),
          .retrieved_words.mean,
          .index_bytes,
          .policy
        ])) as $ranked
      | {run_id:$identity[0].run_id,
         requested_embedding_model:$identity[0].requested_embedding_model,
         actual_embedding_model:$identity[0].actual_embedding_model,
         eligible_policies:[$eligible[].policy],
         material_contenders:[$ranked[].policy],
         max_budgeted_exact_evidence_loss_count:$max_budgeted_loss_count,
         best_budgeted_exact_evidence_hit_count:$best_hit_count,
         selected_policy:$ranked[0].policy,
         ranking_rule:["source_fidelity_pass","within_caller_approved_exact_evidence_loss_count",
           "budgeted_correct_source_recall_desc","duplicate_overlap_rate_asc","exact_evidence_mrr_desc",
           "retrieved_words_mean_asc","index_bytes_asc","policy_asc"],
         policies:$eligible,measured_costs:$costs[0]}
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
