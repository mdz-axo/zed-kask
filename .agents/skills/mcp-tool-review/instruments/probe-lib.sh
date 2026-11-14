#!/usr/bin/env bash
# Isolated stdio session helpers for any kask MCP server probe session.
# Source this file, then: start <binary>; call <tool> <json>; stop; tail_report.
# Isolated: cleared environment; temp HOME and XDG_CONFIG_HOME (the server reads settings.json
# through the config dir and creates that dir at startup); temp data, artifacts and runtime
# dirs; no API keys; no bridge; no model env. XDG_RUNTIME_DIR is the temp dir on purpose
# (kask/crates/hkask-inference/src/inference_ipc_client.rs read_socket_path_from_file falls
# back to {XDG_RUNTIME_DIR or /run/user/<uid>}/kask/inference-socket-path — a cleared
# environment alone still finds the operator's editor bridge).
# Add server-specific env overrides (e.g. HKASK_EXPERIMENTATION_DB) inside start().
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
id=0; RESP=""
start() { coproc SRV { env -i PATH=/usr/bin:/bin HOME="$tmp/home" XDG_CONFIG_HOME="$tmp/config" XDG_RUNTIME_DIR="$tmp" HKASK_DATA_DIR="$tmp/data" HKASK_ARTIFACTS_DIR="$tmp/artifacts" HKASK_DB_INVENTORY_PATH="$tmp/inventory.jsonl" HKASK_DB_PASSPHRASE=mcp-tool-review-probe-only "$1" 2>>"$tmp/stderr.log"; }
  printf '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"mcp-tool-review-probe","version":"0"}}}\n' >&"${SRV[1]}"
  IFS= read -r -t 30 _ <&"${SRV[0]}"
  printf '{"jsonrpc":"2.0","method":"notifications/initialized"}\n' >&"${SRV[1]}"; }
stop() { eval "exec ${SRV[1]}>&-"; wait "$SRV_PID" 2>/dev/null; }
raw() { id=$((id+1)); RESP=""
  printf '{"jsonrpc":"2.0","id":%d,"method":"%s","params":%s}\n' "$id" "$1" "$2" >&"${SRV[1]}"
  while IFS= read -r -t 60 line <&"${SRV[0]}"; do
    if [ "$(jq -r '.id // empty' <<<"$line" 2>/dev/null)" = "$id" ]; then RESP="$line"; break; fi; done; }
call() { raw tools/call "{\"name\":\"$1\",\"arguments\":$2}"; }
get() { jq -r "(.result.content[0].text | fromjson | .content) | $1" <<<"$RESP" 2>/dev/null; }
err() { jq -r 'if (.result.isError // false) or (.error != null) then "ERR: " + (.result.content[0].text // .error.message) else "ok" end' <<<"$RESP" | cut -c1-240; }
tail_report() { echo "--- calls: $id; bridge lines in stderr: $(grep -ci 'bridge' "$tmp/stderr.log"); panic/ERROR lines: $(grep -ci 'panic\| ERROR ' "$tmp/stderr.log"); files created under the temp dir: $(find "$tmp" -type f -not -name stderr.log | wc -l) [$(find "$tmp" -type f -not -name stderr.log | sed "s#$tmp/##" | tr '\n' ' ')]"; }
