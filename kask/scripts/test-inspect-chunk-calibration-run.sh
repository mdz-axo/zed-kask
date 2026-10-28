#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
inspector="$repo_root/kask/scripts/audit/inspect-chunk-calibration-run.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
run="$tmp/run"
mkdir -p "$run/embed/checkpoints"

publish_hashed_json() {
    local path=$1
    jq -cS . "$path" > "$path.sorted"
    mv "$path.sorted" "$path"
    sha256sum "$path" | cut -d' ' -f1 > "$path.sha256"
}

cat > "$run/run-preseal-identity.json" <<'JSON'
{
  "schema_version": 2,
  "run_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "requested_embedding_model": "provider/requested-model",
  "shards": {
    "reference": [{"name":"shard-00000","rows":3,"sha256":"1111111111111111111111111111111111111111111111111111111111111111"}],
    "current": [{"name":"shard-00000","rows":2,"sha256":"2222222222222222222222222222222222222222222222222222222222222222"}],
    "fine": [{"name":"shard-00000","rows":4,"sha256":"3333333333333333333333333333333333333333333333333333333333333333"}]
  }
}
JSON
publish_hashed_json "$run/run-preseal-identity.json"

cat > "$run/embed/checkpoints/reference-shard-00000.json" <<'JSON'
{"schema_version":1,"preseal_run_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","policy":"reference","shard":"shard-00000","requested_model":"provider/requested-model","actual_model":"provider/actual-model","embedded_rows":3,"attempted_rows":3,"recovered_rows":0,"attempts":1,"unmeasured_interrupted_attempts":0,"complete":true}
JSON
publish_hashed_json "$run/embed/checkpoints/reference-shard-00000.json"
cat > "$run/embed/checkpoints/current-shard-00000.json" <<'JSON'
{"schema_version":1,"preseal_run_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","policy":"current","shard":"shard-00000","requested_model":"provider/requested-model","actual_model":"provider/actual-model","embedded_rows":2,"attempted_rows":3,"recovered_rows":0,"attempts":2,"unmeasured_interrupted_attempts":0,"complete":true}
JSON
publish_hashed_json "$run/embed/checkpoints/current-shard-00000.json"
printf '%s\n' '["fine:missing:1","fine:missing:2"]' > "$run/embed/fine-shard-00000-attempt-01-failed-refs.json"

# expect: I can inspect exact durable corpus-run progress without maintaining a second ledger.
# [P9] Motivating: Homeostatic self-regulation requires observable progress and recovery state.
# [P1] [P3] [P8] Constraining: preserve source identity, composability, and durable provenance.
# pre: a hashed preseal identity and zero or more hashed checkpoint receipts exist.
# post: one read-only JSON projection reports planned/completed rows and shards, retry refs, model, and next unit.
before=$(find "$run" -type f -printf '%p\0' | sort -z | xargs -0 sha256sum | sha256sum)
status=$("$inspector" "$run")
after=$(find "$run" -type f -printf '%p\0' | sort -z | xargs -0 sha256sum | sha256sum)
[[ "$before" == "$after" ]]
jq -e '
  .state == "embedding" and
  .preseal_run_id == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" and
  .requested_model == "provider/requested-model" and
  .actual_models == ["provider/actual-model"] and
  .planned_shards == 3 and .completed_shards == 2 and .remaining_shards == 1 and
  .planned_rows == 9 and .embedded_rows == 5 and .attempted_rows == 6 and
  .attempts == 3 and .recovered_rows == 0 and .pending_retry_refs == 2 and
  .completed_by_policy == {reference:1,current:1,fine:0} and
  .next_unit == {policy:"fine",shard:"shard-00000"}
' <<<"$status" >/dev/null

printf '%s\n' '{}' > "$run/comparison.json"
printf '%s\n' '{}' > "$run/run-identity.json"
printf '%s\n' '{}' > "$run/measured-costs.json"
if "$inspector" "$run" >/dev/null; then
    echo "inspector accepted final artifacts before every sealed shard completed" >&2
    exit 1
fi
rm "$run/comparison.json" "$run/run-identity.json" "$run/measured-costs.json"

cp "$run/embed/checkpoints/current-shard-00000.json" "$tmp/checkpoint.original"
jq '.embedded_rows = 0' "$tmp/checkpoint.original" > "$run/embed/checkpoints/current-shard-00000.json"
if "$inspector" "$run" >/dev/null; then
    echo "inspector accepted a checkpoint whose hash sidecar no longer matched" >&2
    exit 1
fi

printf '%s\n' "inspect chunk calibration run test passed"
