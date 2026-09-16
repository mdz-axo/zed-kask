#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
    echo "usage: $0 <tool-name> <arguments-json> <response-json> <server-log>" >&2
    exit 64
fi

tool_name=$1
arguments_file=$2
response_file=$3
log_file=$4
binary=${HKASK_CORPUS_BINARY:-$HOME/.local/bin/hkask-mcp-corpus}

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

host_pid=$(pgrep -f '^hkask-mcp-corpus$' | head -1)
if [[ ! "$host_pid" =~ ^[0-9]+$ ]]; then
    echo "running host-managed hkask-mcp-corpus process not found" >&2
    exit 69
fi

read_host_env() {
    local name=$1
    tr '\0' '\n' < "/proc/$host_pid/environ" | sed -n "s/^${name}=//p"
}

HKASK_INFERENCE_SOCKET=$(read_host_env HKASK_INFERENCE_SOCKET)
HKASK_INFERENCE_TIMEOUT_SECS=$(read_host_env HKASK_INFERENCE_TIMEOUT_SECS)
HKASK_EMBEDDING_MODEL=$(read_host_env HKASK_EMBEDDING_MODEL)
DEEPINFRA_TOKEN=$(read_host_env DEEPINFRA_TOKEN)
export HKASK_INFERENCE_SOCKET HKASK_INFERENCE_TIMEOUT_SECS HKASK_EMBEDDING_MODEL DEEPINFRA_TOKEN
if [[ -z "$HKASK_INFERENCE_SOCKET" || -z "$HKASK_EMBEDDING_MODEL" ]]; then
    echo "host corpus inference configuration is incomplete" >&2
    exit 69
fi

coproc CORPUS_MCP { "$binary" 2>"$log_file"; }
out_fd=${CORPUS_MCP[0]}
in_fd=${CORPUS_MCP[1]}

printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"chunk-calibration","version":"1"}}}' >&"$in_fd"
IFS= read -r initialize_response <&"$out_fd"
jq -e '.id == 1 and .result.protocolVersion == "2025-06-18"' <<<"$initialize_response" >/dev/null
printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' >&"$in_fd"
jq -cn --arg name "$tool_name" --slurpfile arguments "$arguments_file" \
    '{jsonrpc:"2.0",id:2,method:"tools/call",params:{name:$name,arguments:$arguments[0]}}' >&"$in_fd"

found=false
while IFS= read -r line <&"$out_fd"; do
    if jq -e '.id == 2' <<<"$line" >/dev/null 2>&1; then
        printf '%s\n' "$line" > "$response_file"
        found=true
        break
    fi
done

eval "exec ${in_fd}>&-"
wait "$CORPUS_MCP_PID" || true
if [[ "$found" != true ]]; then
    echo "corpus MCP ended without the tool response" >&2
    exit 70
fi
if jq -e '.result.isError == true or .error != null' "$response_file" >/dev/null; then
    echo "corpus tool returned an error; inspect $response_file and $log_file" >&2
    exit 1
fi
printf 'tool=%s response=%s log=%s\n' "$tool_name" "$response_file" "$log_file" >&2
