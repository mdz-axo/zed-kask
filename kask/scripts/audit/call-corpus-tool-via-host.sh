#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
    echo "usage: $0 <corpus_build_chunk_representations|corpus_embedding_inventory|corpus_embed|corpus_query|corpus_tag_chunks|corpus_generate_qa_batch> <arguments-json> <response-json> <server-log>" >&2
    exit 64
fi

tool_name=$1
arguments_file=$2
response_file=$3
log_file=$4
binary=${HKASK_CORPUS_BINARY:-$HOME/.local/bin/hkask-mcp-corpus}
required_secondary_model_var=

case "$tool_name" in
    corpus_build_chunk_representations|corpus_embedding_inventory)
        needs_inference=false
        required_model_var=
        default_timeout=120
        ;;
    corpus_embed|corpus_query)
        needs_inference=true
        required_model_var=HKASK_EMBEDDING_MODEL
        default_timeout=$(( ${HKASK_INFERENCE_TIMEOUT_SECS:-600} + 30 ))
        ;;
    corpus_tag_chunks)
        needs_inference=true
        required_model_var=HKASK_CLASSIFIER_MODEL
        default_timeout=$(( ${HKASK_INFERENCE_TIMEOUT_SECS:-600} + 30 ))
        ;;
    corpus_generate_qa_batch)
        needs_inference=true
        required_model_var=HKASK_QA_GENERATION_MODEL
        required_secondary_model_var=HKASK_QA_VERIFICATION_MODEL
        default_timeout=$(( ${HKASK_INFERENCE_TIMEOUT_SECS:-600} + 30 ))
        ;;
    *)
        echo "unsupported calibration corpus operation: $tool_name" >&2
        exit 64
        ;;
esac

if ! command -v jq >/dev/null 2>&1; then
    echo "required command not found: jq" >&2
    exit 69
fi
for path in "$arguments_file" "$binary"; do
    if [[ ! -f "$path" ]]; then
        echo "required file does not exist: $path" >&2
        exit 66
    fi
done
for path in "$response_file" "$log_file"; do
    if [[ -e "$path" ]]; then
        echo "refusing to overwrite output: $path" >&2
        exit 73
    fi
    mkdir -p "$(dirname "$path")"
done
jq -e 'type == "object"' "$arguments_file" >/dev/null

required_model=
if [[ -n "$required_model_var" ]]; then
    required_model=${!required_model_var:-}
fi
required_secondary_model=
if [[ -n "$required_secondary_model_var" ]]; then
    required_secondary_model=${!required_secondary_model_var:-}
fi
if [[ "$needs_inference" == true && ( -z ${HKASK_INFERENCE_SOCKET:-} || -z $required_model || ( -n $required_secondary_model_var && -z $required_secondary_model ) ) ]]; then
    host_pid=$(pgrep -f '^hkask-mcp-corpus$' | head -1)
    if [[ ! "$host_pid" =~ ^[0-9]+$ ]]; then
        echo "running host-managed hkask-mcp-corpus process not found" >&2
        exit 69
    fi
    read_host_env() {
        local name=$1
        tr '\0' '\n' < "/proc/$host_pid/environ" | sed -n "s/^${name}=//p"
    }
    host_inference_socket=$(read_host_env HKASK_INFERENCE_SOCKET)
    host_inference_timeout=$(read_host_env HKASK_INFERENCE_TIMEOUT_SECS)
    host_embedding_model=$(read_host_env HKASK_EMBEDDING_MODEL)
    host_classifier_model=$(read_host_env HKASK_CLASSIFIER_MODEL)
    host_qa_generation_model=$(read_host_env HKASK_QA_GENERATION_MODEL)
    host_qa_verification_model=$(read_host_env HKASK_QA_VERIFICATION_MODEL)
    host_template_root=$(read_host_env HKASK_TEMPLATE_ROOT)
    host_deepinfra_token=$(read_host_env DEEPINFRA_TOKEN)
    host_openrouter_token=$(read_host_env OPENROUTER_API_KEY)
    HKASK_INFERENCE_SOCKET=${HKASK_INFERENCE_SOCKET:-$host_inference_socket}
    HKASK_INFERENCE_TIMEOUT_SECS=${HKASK_INFERENCE_TIMEOUT_SECS:-$host_inference_timeout}
    HKASK_EMBEDDING_MODEL=${HKASK_EMBEDDING_MODEL:-$host_embedding_model}
    HKASK_CLASSIFIER_MODEL=${HKASK_CLASSIFIER_MODEL:-$host_classifier_model}
    HKASK_QA_GENERATION_MODEL=${HKASK_QA_GENERATION_MODEL:-$host_qa_generation_model}
    HKASK_QA_VERIFICATION_MODEL=${HKASK_QA_VERIFICATION_MODEL:-$host_qa_verification_model}
    HKASK_TEMPLATE_ROOT=${HKASK_TEMPLATE_ROOT:-$host_template_root}
    DEEPINFRA_TOKEN=${DEEPINFRA_TOKEN:-$host_deepinfra_token}
    OPENROUTER_API_KEY=${OPENROUTER_API_KEY:-$host_openrouter_token}
    export HKASK_INFERENCE_SOCKET HKASK_INFERENCE_TIMEOUT_SECS HKASK_EMBEDDING_MODEL
    export HKASK_CLASSIFIER_MODEL HKASK_QA_GENERATION_MODEL HKASK_QA_VERIFICATION_MODEL
    export HKASK_TEMPLATE_ROOT
    export DEEPINFRA_TOKEN OPENROUTER_API_KEY
fi
if [[ -n "$required_model_var" ]]; then
    required_model=${!required_model_var:-}
fi
if [[ "$needs_inference" == true && ( -z ${HKASK_INFERENCE_SOCKET:-} || -z $required_model ) ]]; then
    echo "$tool_name requires HKASK_INFERENCE_SOCKET and $required_model_var" >&2
    exit 69
fi
if [[ "$tool_name" == corpus_tag_chunks && -z ${HKASK_TEMPLATE_ROOT:-} ]]; then
    echo "$tool_name requires HKASK_TEMPLATE_ROOT" >&2
    exit 69
fi

response_timeout=${HKASK_CALIBRATION_RESPONSE_TIMEOUT_SECS:-$default_timeout}
if [[ ! "$response_timeout" =~ ^[1-9][0-9]*$ ]] || (( response_timeout > 3600 )); then
    echo "HKASK_CALIBRATION_RESPONSE_TIMEOUT_SECS must be an integer from 1 through 3600" >&2
    exit 64
fi
child_term_grace=${HKASK_CALIBRATION_CHILD_TERM_GRACE_SECS:-5}
if [[ ! "$child_term_grace" =~ ^[1-9][0-9]*$ ]] || (( child_term_grace > 30 )); then
    echo "HKASK_CALIBRATION_CHILD_TERM_GRACE_SECS must be an integer from 1 through 30" >&2
    exit 64
fi

coproc CORPUS_MCP { exec "$binary" 2>"$log_file"; }
out_fd=${CORPUS_MCP[0]}
in_fd=${CORPUS_MCP[1]}
corpus_pid=$CORPUS_MCP_PID
process_stopped() {
    local pid=$1 state
    if ! kill -0 "$pid" 2>/dev/null; then
        return 0
    fi
    state=$(ps -o stat= -p "$pid" 2>/dev/null || true)
    [[ -z "$state" || "$state" == Z* ]]
}
wait_for_stop() {
    local pid=$1 limit=$2 elapsed=0
    while ! process_stopped "$pid" && (( elapsed < limit * 10 )); do
        sleep 0.1
        elapsed=$((elapsed + 1))
    done
    process_stopped "$pid"
}
cleanup() {
    local child_pid=${corpus_pid:-}
    corpus_pid=
    if [[ -n ${in_fd:-} ]]; then
        eval "exec ${in_fd}>&-" 2>/dev/null || true
        in_fd=
    fi
    if [[ -n ${out_fd:-} ]]; then
        eval "exec ${out_fd}<&-" 2>/dev/null || true
        out_fd=
    fi
    if [[ -n "$child_pid" ]]; then
        if ! process_stopped "$child_pid"; then
            kill -TERM "$child_pid" 2>/dev/null || true
            if ! wait_for_stop "$child_pid" "$child_term_grace"; then
                echo "corpus MCP child did not stop after ${child_term_grace}s; sending KILL: pid=$child_pid" >&2
                kill -KILL "$child_pid" 2>/dev/null || true
                wait_for_stop "$child_pid" "$child_term_grace" || \
                    echo "corpus MCP child remained uninterruptible after KILL: pid=$child_pid" >&2
            fi
        fi
        if process_stopped "$child_pid"; then
            wait "$child_pid" 2>/dev/null || true
        fi
    fi
}
trap cleanup EXIT

read_response() {
    local expected_id=$1
    local line
    while IFS= read -r -t "$response_timeout" line <&"$out_fd"; do
        if jq -e --argjson id "$expected_id" '.id == $id' <<<"$line" >/dev/null 2>&1; then
            printf '%s\n' "$line"
            return 0
        fi
    done
    echo "corpus MCP ended or timed out before response id $expected_id" >&2
    return 70
}

printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"chunk-calibration","version":"1"}}}' >&"$in_fd"
initialize_response=$(read_response 1)
jq -e '.result.protocolVersion == "2025-06-18" and .error == null' <<<"$initialize_response" >/dev/null
printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' >&"$in_fd"
jq -cn --arg name "$tool_name" --slurpfile arguments "$arguments_file" \
    '{jsonrpc:"2.0",id:2,method:"tools/call",params:{name:$name,arguments:$arguments[0]}}' >&"$in_fd"
read_response 2 > "$response_file"

cleanup
trap - EXIT
if jq -e '.result.isError == true or .error != null' "$response_file" >/dev/null; then
    echo "corpus tool returned an error; inspect $response_file and $log_file" >&2
    exit 1
fi
printf 'tool=%s response=%s log=%s\n' "$tool_name" "$response_file" "$log_file" >&2
