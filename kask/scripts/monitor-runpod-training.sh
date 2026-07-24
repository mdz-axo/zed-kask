#!/usr/bin/env bash
# monitor-runpod-training.sh — Live monitor for an hKask RunPod axolotl training job.
#
# Polls the RunPod GraphQL API for pod lifecycle state + GPU/CPU/memory telemetry,
# and periodically calls the governed `training_status` MCP tool (on the
# hkask-mcp-training binary) to capture the hKask job lifecycle, including
# automatic adapter registration on completion.
#
# Why telemetry, not stdout logs: RunPod's public GraphQL/REST API does NOT
# expose pod container (stdout) logs — those are only viewable in the web
# console. The `latestTelemetry` + `runtime` fields are the API-accessible
# proxy for "where in the process": GPU util/mem/temp/power disambiguate the
# phases (idle init → model/dataset download → active training → done).
#
# Usage:
#   scripts/monitor-runpod-training.sh <pod_id> [job_id] [interval_secs] [max_runtime_secs]
#
#   pod_id           RunPod pod ID (required). Example: g6l1spp63u4drp
#   job_id           hKask training job UUID (optional). If set, the governed
#                    training_status MCP tool is polled every --hkask-every polls
#                    to track job lifecycle + auto-registration. Defaults to $HKASK_JOB_ID.
#   interval_secs    Poll interval in seconds (default 30).
#   max_runtime_secs  Safety cap so the monitor can't run forever (default 86400 = 24h).
#
# Credentials: reads RUNPOD_API_KEY from the environment. If unset, falls back
# to reading it from ./.env (read-only — never modifies .env). Requires the
# hkask-mcp-training binary at ./target/{debug,release}/hkask-mcp-training for
# the governed status poll (skipped silently if absent).
#
# Exit codes: 0 on terminal pod state (EXITED/DEAD/TERMINATED) or max_runtime;
#             1 on auth/argument errors. Ctrl+C stops cleanly.
set -u

POD_ID="${1:-}"
JOB_ID="${2:-${HKASK_JOB_ID:-}}"
INTERVAL="${3:-30}"
MAX_RUNTIME="${4:-86400}"
HKASK_EVERY_POLLS=10  # governed training_status poll cadence (every N polls)
GRAPHQL="https://api.runpod.io/graphql"
UA="hkask-monitor/0.1"

if [[ -z "$POD_ID" ]]; then
  echo "usage: $0 <pod_id> [job_id] [interval_secs] [max_runtime_secs]" >&2
  exit 1
fi

# ── Resolve RUNPOD_API_KEY (env first, then ./.env read-only) ───────────────
if [[ -z "${RUNPOD_API_KEY:-}" ]]; then
  if [[ -f ./.env ]]; then
    RUNPOD_API_KEY="$(grep '^RUNPOD_API_KEY=' ./.env | head -1 | cut -d= -f2-)"
  fi
fi
if [[ -z "${RUNPOD_API_KEY:-}" ]]; then
  echo "error: RUNPOD_API_KEY not set and not found in ./.env" >&2
  exit 1
fi
export RUNPOD_API_KEY

# ── Locate the governed training binary (optional) ──────────────────────────
TRAIN_BIN=""
for cand in ./target/release/hkask-mcp-training ./target/debug/hkask-mcp-training; do
  if [[ -x "$cand" ]]; then TRAIN_BIN="$cand"; break; fi
done

# Pod query: lifecycle + runtime + telemetry. Fields verified against the
# RunPod GraphQL spec (Pod.latestTelemetry, Pod.runtime.{uptimeInSeconds,
# container, gpus}, Pod.machine.{gpuTypeId,gpuDisplayName}).
read -r -d '' POD_QUERY <<'EOF'
query($id:String!){pod(input:{podId:$id}){id name desiredStatus lastStatusChange costPerHr imageName runtime{uptimeInSeconds container{cpuPercent memoryPercent} gpus{id gpuUtilPercent memoryUtilPercent}} latestTelemetry{state time cpuUtilization memoryUtilization lastStateTransitionTimestamp averageGpuMetrics{percentUtilization memoryUtilization temperatureCelcius powerWatts}} machine{gpuTypeId gpuDisplayName}}}
EOF

fmt_dur() { # seconds -> "Xh Ym Zs"; clamps RunPod's negative-uptime quirk
  local s=$1 d h m
  if (( s < 0 )); then printf "<1s (just restarted)"; return; fi
  d=$((s/86400)); s=$((s%86400)); h=$((s/3600)); s=$((s%3600)); m=$((s/60)); s=$((s%60))
  if (( d>0 )); then printf "%dd %dh %dm" "$d" "$h" "$m"
  elif (( h>0 )); then printf "%dh %dm %ds" "$h" "$m" "$s"
  elif (( m>0 )); then printf "%dm %ds" "$m" "$s"
  else printf "%ds" "$s"; fi
}

fetch_pod() {
  local body
  body=$(jq -n --arg q "$POD_QUERY" --arg id "$POD_ID" '{query:$q, variables:{id:$id}}')
  curl -sS --max-time 20 -X POST "$GRAPHQL" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer $RUNPOD_API_KEY" \
    -H "User-Agent: $UA" \
    -d "$body"
}

# Governed hKask job status via the MCP binary's stdio protocol. Single tool
# call per invocation; prints the mapped lifecycle state. This is the same
# governed seam used at submission (not a direct RunPod call).
governed_status() {
  [[ -z "$TRAIN_BIN" || -z "$JOB_ID" ]] && return 0
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"hkask-monitor","version":"0.1"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}' \
    "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"training_status\",\"arguments\":{\"job_id\":\"$JOB_ID\"}}}" \
    | timeout 60 "$TRAIN_BIN" 2>/dev/null \
    | jq -r 'select(.id==2) | .result.content[0].text' 2>/dev/null \
    | jq -r '"  [governed] job_id="+(.content.job_id//"?")+" status="+(.content.status//"?")+(if .content.adapter_registered==true then " adapter_registered=true" else "" end)' 2>/dev/null
}

echo "=== hKask RunPod training monitor ==="
echo "pod_id=$POD_ID job_id=${JOB_ID:-<none>} interval=${INTERVAL}s max_runtime=$(fmt_dur "$MAX_RUNTIME")"
echo "train_bin=${TRAIN_BIN:-<none — governed status disabled>}"
echo "columns: time | status | up | GPU util%/mem%/tempC/W | CPU%/mem% | $/hr | phase"
echo

start=$(date +%s); poll=0; prev_status=""
while true; do
  now=$(date +%s); elapsed=$((now-start))
  if (( elapsed >= MAX_RUNTIME )); then echo "[max_runtime reached after $(fmt_dur "$elapsed")]"; exit 0; fi

  resp=$(fetch_pod)
  if [[ -z "$resp" ]]; then echo "$(date -u +%H:%M:%S) | <no response> "; sleep "$INTERVAL"; continue; fi

  # Pull fields with jq; tolerate missing telemetry (container still booting).
  status=$(jq -r '.data.pod.desiredStatus // "UNKNOWN"' <<<"$resp")
  up=$(jq -r '.data.pod.runtime.uptimeInSeconds // 0' <<<"$resp")
  gpu_util=$(jq -r '.data.pod.latestTelemetry.averageGpuMetrics.percentUtilization // .data.pod.runtime.gpus[0].gpuUtilPercent // 0' <<<"$resp")
  gpu_mem=$(jq -r '.data.pod.latestTelemetry.averageGpuMetrics.memoryUtilization // .data.pod.runtime.gpus[0].memoryUtilPercent // 0' <<<"$resp")
  gpu_temp=$(jq -r '.data.pod.latestTelemetry.averageGpuMetrics.temperatureCelcius // 0' <<<"$resp")
  gpu_w=$(jq -r '.data.pod.latestTelemetry.averageGpuMetrics.powerWatts // 0' <<<"$resp")
  cpu=$(jq -r '.data.pod.latestTelemetry.cpuUtilization // .data.pod.runtime.container.cpuPercent // 0' <<<"$resp")
  mem=$(jq -r '.data.pod.latestTelemetry.memoryUtilization // .data.pod.runtime.container.memoryPercent // 0' <<<"$resp")
  cost=$(jq -r '.data.pod.costPerHr // 0' <<<"$resp")

  # Phase heuristic from telemetry (GPU util is the key training signal).
  if   (( gpu_util >= 50 )); then phase="training"
  elif (( gpu_util >= 5 ));  then phase="training(warmup)"
  elif (( cpu >= 30 || mem >= 30 )); then phase="download/init"
  else phase="idle/boot"; fi

  line="$(date -u +%H:%M:%S) | $status | $(fmt_dur "$up") | ${gpu_util}%/${gpu_mem}%/${gpu_temp}C/${gpu_w}W | ${cpu}%/${mem}% | \$${cost}/hr | $phase"

  if [[ "$status" != "$prev_status" ]]; then
    [[ -n "$prev_status" ]] && echo "── transition: $prev_status → $status ──"
    prev_status="$status"
  fi
  echo "$line"

  # Governed hKask job lifecycle poll (every HKASK_EVERY_POLLS).
  poll=$((poll+1))
  if (( poll % HKASK_EVERY_POLLS == 0 )); then governed_status; fi

  # Terminal pod states → do a final governed status (captures auto-registration) and stop.
  case "$status" in
    EXITED|DEAD|TERMINATED)
      echo "── pod reached terminal state: $status ──"
      governed_status
      exit 0 ;;
  esac

  sleep "$INTERVAL"
done