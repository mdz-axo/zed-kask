#!/usr/bin/env bash
# p2-parity-longleg.sh — the P2 long-generation leg (the divergence class's
# primary evidence: streamed-vs-resident divergence shows on long generations —
# the public-retraction lesson, wrapper plan §2). The frozen p0-bench-llama.sh
# suite runs 128/256/256-token completions; this runner adds ONLY the long leg:
# the frozen P2 prompt at max_tokens 1024, temperature 0, three reps, one
# discarded warmup, the same evidence discipline (bodies + timings + the
# server-log slice + RAM/VRAM samples per request).
#
# Like p0-bench-llama.sh, this script only measures an ALREADY-READY
# llama-server (the leg is launched BY HAND per the P0 discipline — the launch
# flags, including the fresh-slot protocol, are recorded with the leg in the
# P2 plan §9 record, never assumed by this script). The prompt bytes come from
# the one frozen home via extract-frozen-p2-prompt.sh — this runner never
# invents prompt bytes.
#
# Usage: p2-parity-longleg.sh ENDPOINT SERVER_PID SERVER_LOG OUT_DIR LABEL
set -euo pipefail

if [ "$#" -ne 5 ]; then
    echo "usage: $0 ENDPOINT SERVER_PID SERVER_LOG OUT_DIR LABEL" >&2
    exit 2
fi

endpoint=$1
server_pid=$2
server_log=$3
out=$4
label=$5

mkdir -p "$out"

json_escape() {
    local s=$1
    s=${s//\\/\\\\}
    s=${s//\"/\\\"}
    s=${s//$'\n'/\\n}
    s=${s//$'\t'/\\t}
    s=${s//$'\r'/\\r}
    printf '%s' "$s"
}

# The system prompt: the same byte-frozen SYS line as the p0 suite (verified
# one-line copy — the compare step refuses legs whose request bodies drift
# from the shared definition).
SYS="You are a senior Rust systems engineer working inside a private, local-only assistant environment. Answer with precision and brevity. When reviewing code, name concrete, actionable improvements; never speculate about code you cannot see."

SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
P2_FULL=$(bash "$SCRIPTS_DIR/extract-frozen-p2-prompt.sh") || {
    echo "FATAL: the frozen P2 prompt could not be extracted — do not substitute bytes" >&2
    exit 3
}

echo "# p2-parity-longleg label=$label start $(date -Is) endpoint=$endpoint" > "$out/longleg.meta"

build_request() {
    local prompt=$1 max_tokens=$2 outfile=$3
    printf '{"messages":[{"role":"system","content":"%s"},{"role":"user","content":"%s"}],"temperature":0.0,"max_tokens":%s,"stream":false}' \
        "$(json_escape "$SYS")" "$(json_escape "$prompt")" "$max_tokens" \
        > "$outfile"
}

sample_mem() {
    local what=$1 outfile=$2
    {
        echo "# $what $(date -Is)"
        if [ -r "/proc/$server_pid/status" ]; then
            echo "vm_rss_kb=$(awk '/^VmRSS:/ {print $2}' "/proc/$server_pid/status")"
            echo "vm_hwm_kb=$(awk '/^VmHWM:/ {print $2}' "/proc/$server_pid/status")"
        fi
        echo "mem_available_kb=$(awk '/^MemAvailable:/ {print $2}' /proc/meminfo)"
        for f in /sys/class/drm/card*/device/mem_info_vram_used; do
            [ -r "$f" ] && echo "$(basename "$(dirname "$(dirname "$f")")")_vram_used_bytes=$(cat "$f")"
        done
    } >> "$outfile"
}

req() {
    local class=$1 rep=$2 body=$3 timing=$4
    local t0 t1
    t0=$(date +%s.%N)
    curl -sS --max-time 1800 "$endpoint/v1/chat/completions" -H 'Content-Type: application/json' -d @"$body" > "$out/${class}_r${rep}.json"
    t1=$(date +%s.%N)
    awk -v a="$t0" -v b="$t1" 'BEGIN {printf "%.3f\n", b-a}' > "$timing"
    echo "--- ${class} r${rep} (label=$label) bytes=$(wc -c < "$out/${class}_r${rep}.json") ---" >> "$out/server-log.slices"
    tail -c 4000 "$server_log" >> "$out/server-log.slices" 2>/dev/null || true
}

# warmup (discarded)
build_request "$P2_FULL" 8 "$out/warmup.req.json"
curl -sS --max-time 300 "$endpoint/v1/chat/completions" -H 'Content-Type: application/json' -d @"$out/warmup.req.json" > /dev/null

sample_mem "post_warmup" "$out/mem.log"

for rep in 1 2 3; do
    build_request "$P2_FULL" 1024 "$out/long_r${rep}.req.json"
    req "long" "$rep" "$out/long_r${rep}.req.json" "$out/long_r${rep}.timing"
    sample_mem "post_long_r${rep}" "$out/mem.log"
done

echo "# done $(date -Is)" >> "$out/longleg.meta"
echo "LONGLEG_DONE $label"
