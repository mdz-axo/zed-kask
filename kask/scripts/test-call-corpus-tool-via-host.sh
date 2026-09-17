#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
host_call="$repo_root/kask/scripts/audit/call-corpus-tool-via-host.sh"
tmp=$(mktemp -d)
cleanup() {
    if [[ -s "$tmp/server.pid" ]]; then
        pid=$(cat "$tmp/server.pid")
        if kill -0 "$pid" 2>/dev/null; then
            kill -KILL "$pid" 2>/dev/null || true
        fi
    fi
    rm -rf "$tmp"
}
trap cleanup EXIT

cat > "$tmp/hanging-corpus" <<'BASH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$$" > "$FAKE_SERVER_PID_FILE"
while IFS= read -r line; do
    method=$(jq -r '.method' <<<"$line")
    if [[ "$method" == initialize ]]; then
        jq -cn --argjson id "$(jq '.id' <<<"$line")" \
            '{jsonrpc:"2.0",id:$id,result:{protocolVersion:"2025-06-18",capabilities:{},serverInfo:{name:"hanging",version:"1"}}}'
    fi
done
while :; do sleep 1; done
BASH
chmod +x "$tmp/hanging-corpus"
printf '%s\n' '{"chunks_jsonl":"unused","tagged_jsonl":null,"db_path":"unused","model":"fixture/model","batch_size":1}' > "$tmp/arguments.json"

export HKASK_CORPUS_BINARY="$tmp/hanging-corpus"
export HKASK_INFERENCE_SOCKET="fixture-socket"
export HKASK_EMBEDDING_MODEL="fixture/model"
export HKASK_INFERENCE_TIMEOUT_SECS=1
export HKASK_CALIBRATION_RESPONSE_TIMEOUT_SECS=1
export HKASK_CALIBRATION_CHILD_TERM_GRACE_SECS=1
export FAKE_SERVER_PID_FILE="$tmp/server.pid"

started=$SECONDS
if "$host_call" corpus_embed "$tmp/arguments.json" "$tmp/response.json" "$tmp/server.log"; then
    echo "host call unexpectedly succeeded without a tool response" >&2
    exit 1
fi
elapsed=$((SECONDS - started))
if (( elapsed > 5 )); then
    echo "host call did not finish bounded cleanup: elapsed=${elapsed}s" >&2
    exit 1
fi
[[ -s "$tmp/server.pid" ]]
server_pid=$(cat "$tmp/server.pid")
if kill -0 "$server_pid" 2>/dev/null; then
    echo "host call left corpus child alive after response timeout: pid=$server_pid" >&2
    exit 1
fi

cat > "$tmp/exiting-corpus" <<'BASH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$$" > "$FAKE_SERVER_PID_FILE"
trap 'touch "$FAKE_SERVER_TERMINATED_FILE"; exit 0' TERM
while IFS= read -r line; do
    method=$(jq -r '.method' <<<"$line")
    case "$method" in
        initialize)
            jq -cn --argjson id "$(jq '.id' <<<"$line")" \
                '{jsonrpc:"2.0",id:$id,result:{protocolVersion:"2025-06-18",capabilities:{},serverInfo:{name:"exiting",version:"1"}}}'
            ;;
        tools/call)
            jq -cn --argjson id "$(jq '.id' <<<"$line")" \
                '{jsonrpc:"2.0",id:$id,result:{content:[{type:"text",text:"{\"content\":{}}"}],isError:false}}'
            ;;
    esac
done
touch "$FAKE_SERVER_GRACEFUL_FILE"
BASH
chmod +x "$tmp/exiting-corpus"
export HKASK_CORPUS_BINARY="$tmp/exiting-corpus"
export FAKE_SERVER_GRACEFUL_FILE="$tmp/graceful-exit"
export FAKE_SERVER_TERMINATED_FILE="$tmp/terminated-exit"
rm -f "$tmp/response.json" "$tmp/server.log"
"$host_call" corpus_embed "$tmp/arguments.json" "$tmp/response.json" "$tmp/server.log"
jq -e '.id == 2 and .result.isError == false' "$tmp/response.json" >/dev/null
[[ -e "$FAKE_SERVER_GRACEFUL_FILE" ]]
[[ ! -e "$FAKE_SERVER_TERMINATED_FILE" ]]
server_pid=$(cat "$tmp/server.pid")
if kill -0 "$server_pid" 2>/dev/null; then
    echo "host call left normally exiting corpus child alive: pid=$server_pid" >&2
    exit 1
fi

export HKASK_QA_GENERATION_MODEL="fixture/qa"
rm -f "$FAKE_SERVER_GRACEFUL_FILE" "$FAKE_SERVER_TERMINATED_FILE"
"$host_call" corpus_generate_qa_batch "$tmp/arguments.json" "$tmp/qa-response.json" "$tmp/qa-server.log"
jq -e '.id == 2 and .result.isError == false' "$tmp/qa-response.json" >/dev/null
[[ -e "$FAKE_SERVER_GRACEFUL_FILE" ]]
[[ ! -e "$FAKE_SERVER_TERMINATED_FILE" ]]
printf '%s\n' "call corpus tool timeout/reap, normal-exit, and QA-routing tests passed"
