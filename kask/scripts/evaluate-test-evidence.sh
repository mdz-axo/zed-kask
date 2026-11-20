#!/usr/bin/env bash
# Explicit local evaluation, not a sandbox or promotion gate. No arbitrary command input.
set -euo pipefail
if (( $# < 8 )); then
    echo 'usage: evaluate-test-evidence.sh CHECKER MANIFEST PACKAGE CONTRACT ORACLE_FILE NEW_OUTPUT WALL_SECONDS INPUT_FILE...' >&2
    exit 2
fi
checker=$(realpath -e "$1")
runner=$(realpath -e "${BASH_SOURCE[0]}")
manifest=$(realpath -e "$2")
package=$3
contract=$(realpath -e "$4")
oracle=$(realpath -e "$5")
output=$6
wall=$7
shift 7
[[ -x "$checker" && -f "$manifest" && -f "$contract" && -f "$oracle" ]]
if [[ ! "$package" =~ ^[a-zA-Z0-9_-]+$ || ! "$wall" =~ ^[1-9][0-9]{0,3}$ ]]; then
    echo 'invalid package or wall budget (integer seconds 1..3600 required)' >&2
    exit 2
fi
if (( wall > 3600 )); then
    echo 'wall budget exceeds 3600 seconds' >&2
    exit 2
fi
for tool in cargo rustc jq sha256sum timeout; do command -v "$tool" >/dev/null; done
root=$(dirname "$manifest")
[[ -f "$root/Cargo.lock" ]]
inputs=("$manifest" "$root/Cargo.lock")
for input in "$@"; do
    input=$(realpath -e "$input")
    [[ -f "$input" && "$input" != *$'\n'* ]]
    inputs+=("$input")
done
umask 077
mkdir "$output" # Never replace a previous run's evidence.
output=$(realpath -e "$output")
cd "$root"
sha256sum "${inputs[@]}" > "$output/inputs.sha256"
sha256sum "$contract" "$oracle" "$checker" "$runner" > "$output/evaluator-inputs.sha256"
command=(cargo test --locked --offline --manifest-path "$manifest" -p "$package" --lib --color never -- --test-threads=1)
{
    printf '%q ' "${command[@]}"
    printf '\nwall_seconds=%s\nbuild_jobs=2\n' "$wall"
    cargo --version
    rustc -vV
    # Fingerprint ambient configuration without writing credentials to evidence.
    env -0 | LC_ALL=C sort -z | sha256sum
    cat "$output/evaluator-inputs.sha256"
} > "$output/evaluator.txt"
artifact=$(sha256sum "$output/inputs.sha256" | cut -d ' ' -f 1)
evaluator=$(sha256sum "$output/evaluator.txt" | cut -d ' ' -f 1)
contract_digest=$(sha256sum "$contract" | cut -d ' ' -f 1)
run_id=$(cat /proc/sys/kernel/random/uuid)
requested=$(date -u +%FT%TZ)
jq -n --arg run "$run_id" --arg artifact "$artifact" --arg evaluator "$evaluator" \
    --arg contract "$contract_digest" --arg author "$(id -un):$(id -u)@$(hostname)" \
    --arg requested "$requested" --argjson age "$((wall + 300))" \
    '{identity:{run_id:$run,artifact_sha256:$artifact,evaluator_sha256:$evaluator,contract_sha256:$contract,author:$author},requested_at:$requested,max_age_seconds:$age}' \
    > "$output/expected.json"
started=$(date -u +%FT%TZ)
status=0
CARGO_BUILD_JOBS=2 timeout -k 5s "${wall}s" "${command[@]}" > "$output/tests.log" 2>&1 || status=$?
finished=$(date -u +%FT%TZ)
unchanged=true
sha256sum --check "$output/inputs.sha256" > "$output/inputs-recheck.log" 2>&1 || unchanged=false
sha256sum --check "$output/evaluator-inputs.sha256" > "$output/evaluator-recheck.log" 2>&1 || unchanged=false
log_digest=$(sha256sum "$output/tests.log" | cut -d ' ' -f 1)
jq --arg started "$started" --arg finished "$finished" --argjson exit "$status" \
    --argjson unchanged "$unchanged" --arg log "$log_digest" \
    '{identity,started_at:$started,finished_at:$finished,exit_code:$exit,inputs_unchanged:$unchanged,log_sha256:$log}' \
    "$output/expected.json" > "$output/report.json"
status=0
"$checker" "$output/expected.json" "$output/report.json" "$output/tests.log" > "$output/receipt.json" || status=$?
cat "$output/receipt.json"
(cd "$output" && sha256sum expected.json report.json receipt.json tests.log evaluator.txt inputs.sha256 evaluator-inputs.sha256 inputs-recheck.log evaluator-recheck.log > SHA256SUMS)
exit "$status"