#!/usr/bin/env bash
set -euo pipefail

# expect: I can inspect exact durable corpus-run progress without maintaining a second ledger.
# [P9] Motivating: Homeostatic self-regulation requires observable progress and recovery state.
# [P1] [P3] [P8] Constraining: preserve source identity, composability, and durable provenance.
# pre: one run output directory exists; post: emit one read-only receipt projection or fail on tampering.

usage() {
    echo "usage: $0 <calibration-output-dir>" >&2
    exit 64
}
[[ $# -eq 1 ]] || usage
run=$1
if [[ ! -d "$run" ]]; then
    echo "calibration output directory does not exist: $run" >&2
    exit 66
fi
for command in jq sha256sum; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "required command not found: $command" >&2
        exit 69
    fi
done

sha_file() {
    sha256sum "$1" | cut -d' ' -f1
}
verify_hashed_json() {
    local path=$1
    if [[ ! -s "$path" || ! -s "$path.sha256" ]]; then
        echo "hashed JSON receipt is incomplete: $path" >&2
        return 66
    fi
    if [[ $(cat "$path.sha256") != "$(sha_file "$path")" ]]; then
        echo "hashed JSON receipt changed: $path" >&2
        return 65
    fi
    jq -e 'type == "object"' "$path" >/dev/null
}

preseal="$run/run-preseal-identity.json"
if [[ ! -e "$preseal" && ! -e "$preseal.sha256" ]]; then
    jq -cn --arg output_dir "$run" '{state:"preparing",output_dir:$output_dir,preseal_run_id:null,
      requested_model:null,actual_models:[],planned_shards:null,completed_shards:0,
      remaining_shards:null,planned_rows:null,embedded_rows:0,attempted_rows:0,
      recovered_rows:0,attempts:0,pending_retry_refs:0,
      completed_by_policy:{reference:0,current:0,fine:0},next_unit:null}'
    exit 0
fi
verify_hashed_json "$preseal"
jq -e '
  . as $root |
  (.run_id | type == "string" and test("^[0-9a-f]{64}$")) and
  (.requested_embedding_model | type == "string" and length > 0) and
  (["reference","current","fine"] | all(.[];
    . as $policy |
    ($root.shards[$policy] | type == "array") and
    all($root.shards[$policy][];
      (.name | type == "string" and test("^shard-[0-9]{5}$")) and
      (.rows | type == "number" and . > 0 and floor == .))))
' "$preseal" >/dev/null
preseal_run_id=$(jq -r '.run_id' "$preseal")
requested_model=$(jq -r '.requested_embedding_model' "$preseal")
planned_shards=$(jq '[.shards.reference,.shards.current,.shards.fine] | map(length) | add' "$preseal")
planned_rows=$(jq '[.shards.reference[],.shards.current[],.shards.fine[]] | map(.rows) | add' "$preseal")

checkpoint_dir="$run/embed/checkpoints"
shopt -s nullglob
checkpoints=("$checkpoint_dir"/*.json)
for checkpoint in "${checkpoints[@]}"; do
    verify_hashed_json "$checkpoint"
    policy=$(jq -r '.policy' "$checkpoint")
    shard=$(jq -r '.shard' "$checkpoint")
    if [[ $(basename "$checkpoint") != "$policy-$shard.json" ]]; then
        echo "checkpoint filename does not match its declared policy/shard: $checkpoint" >&2
        exit 65
    fi
    expected_rows=$(jq -er --arg policy "$policy" --arg shard "$shard" \
        '.shards[$policy][] | select(.name == $shard) | .rows' "$preseal")
    jq -e --arg preseal "$preseal_run_id" --arg policy "$policy" --arg shard "$shard" \
        --argjson rows "$expected_rows" '
      .preseal_run_id == $preseal and .policy == $policy and .shard == $shard and
      .complete == true and .embedded_rows == $rows and
      (.attempted_rows | type == "number" and . >= $rows) and
      (.recovered_rows | type == "number" and . >= 0) and
      (.attempts | type == "number" and . > 0) and
      (.actual_model | type == "string" and length > 0)
    ' "$checkpoint" >/dev/null || {
        echo "checkpoint does not match sealed shard plan: $checkpoint" >&2
        exit 65
    }
done

if (( ${#checkpoints[@]} == 0 )); then
    checkpoint_summary='{"completed_shards":0,"embedded_rows":0,"attempted_rows":0,"recovered_rows":0,"attempts":0,"actual_models":[],"completed_by_policy":{"reference":0,"current":0,"fine":0}}'
else
    checkpoint_summary=$(jq -s '
      {completed_shards:length,
       embedded_rows:(map(.embedded_rows)|add),attempted_rows:(map(.attempted_rows)|add),
       recovered_rows:(map(.recovered_rows)|add),attempts:(map(.attempts)|add),
       actual_models:(map(.actual_model)|unique),
       completed_by_policy:(reduce .[] as $row ({reference:0,current:0,fine:0}; .[$row.policy] += 1))}
    ' "${checkpoints[@]}")
fi
completed_shards=$(jq -r '.completed_shards' <<<"$checkpoint_summary")
remaining_shards=$((planned_shards - completed_shards))
if (( remaining_shards < 0 )); then
    echo "checkpoint count exceeds sealed shard plan" >&2
    exit 65
fi
if (( $(jq '.actual_models | length' <<<"$checkpoint_summary") > 1 )); then
    echo "checkpoint receipts contain more than one provider-confirmed actual model" >&2
    exit 65
fi

pending_retry_refs=0
next_unit=null
for policy in reference current fine; do
    while IFS= read -r shard; do
        checkpoint="$checkpoint_dir/$policy-$shard.json"
        if [[ -e "$checkpoint" ]]; then
            continue
        fi
        if [[ "$next_unit" == null ]]; then
            next_unit=$(jq -cn --arg policy "$policy" --arg shard "$shard" '{policy:$policy,shard:$shard}')
        fi
        failed_receipts=("$run/embed/$policy-$shard-attempt-"*-failed-refs.json)
        if (( ${#failed_receipts[@]} > 0 )); then
            latest=${failed_receipts[${#failed_receipts[@]}-1]}
            jq -e 'type == "array" and all(.[]; type == "string" and length > 0)' "$latest" >/dev/null
            pending_retry_refs=$((pending_retry_refs + $(jq 'length' "$latest")))
        fi
    done < <(jq -r --arg policy "$policy" '.shards[$policy][].name' "$preseal")
done

state=embedding
if (( completed_shards == planned_shards )); then
    state=evaluating
fi
final_artifacts=0
for final_path in "$run/comparison.json" "$run/run-identity.json" "$run/measured-costs.json"; do
    [[ -e "$final_path" ]] && final_artifacts=$((final_artifacts + 1))
done
if (( final_artifacts > 0 && completed_shards != planned_shards )); then
    echo "final artifacts exist before every sealed shard completed" >&2
    exit 65
fi
if (( completed_shards == planned_shards )); then
    actual_model=$(jq -er '.actual_models[0]' <<<"$checkpoint_summary")
    if [[ -e "$run/run-identity.json" ]]; then
        jq -e --arg preseal "$preseal_run_id" --arg requested "$requested_model" --arg actual "$actual_model" '
          .preseal_run_id == $preseal and .requested_embedding_model == $requested and
          .actual_embedding_model == $actual
        ' "$run/run-identity.json" >/dev/null
    fi
    if [[ -e "$run/measured-costs.json" ]]; then
        jq -e '(.embedding | type == "array" and length == 3)' "$run/measured-costs.json" >/dev/null
    fi
    if [[ -e "$run/comparison.json" ]]; then
        if (( final_artifacts != 3 )); then
            echo "comparison exists without every required final artifact" >&2
            exit 65
        fi
        jq -e --arg requested "$requested_model" --arg actual "$actual_model" '
          .requested_embedding_model == $requested and .actual_embedding_model == $actual and
          (.selected_policy | type == "string" and length > 0) and
          (.policies | type == "array" and length == 3)
        ' "$run/comparison.json" >/dev/null
        jq -e '(.evaluation | type == "array" and length == 3)' "$run/measured-costs.json" >/dev/null
        state=complete
    fi
fi

jq -cn --arg state "$state" --arg output_dir "$run" --arg preseal "$preseal_run_id" \
    --arg requested "$requested_model" --argjson planned_shards "$planned_shards" \
    --argjson remaining_shards "$remaining_shards" --argjson planned_rows "$planned_rows" \
    --argjson pending_retry_refs "$pending_retry_refs" --argjson next_unit "$next_unit" \
    --argjson summary "$checkpoint_summary" '
  {state:$state,output_dir:$output_dir,preseal_run_id:$preseal,requested_model:$requested,
   actual_models:$summary.actual_models,planned_shards:$planned_shards,
   completed_shards:$summary.completed_shards,remaining_shards:$remaining_shards,
   planned_rows:$planned_rows,embedded_rows:$summary.embedded_rows,
   attempted_rows:$summary.attempted_rows,recovered_rows:$summary.recovered_rows,
   attempts:$summary.attempts,pending_retry_refs:$pending_retry_refs,
   completed_by_policy:$summary.completed_by_policy,next_unit:$next_unit}'
