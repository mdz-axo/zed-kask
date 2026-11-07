#!/usr/bin/env bash
# determinism-study-leg.sh — the capture runner for the engine-compute
# determinism study (the pre-registered design:
# kask/docs/research/private-inference-engine-compute-determinism-design-2026-11-06.md;
# executed under the plan doc of the same date). Like p0-bench-llama.sh and
# p2-parity-longleg.sh, this script only measures an ALREADY-READY,
# hand-launched llama-server (the P0 discipline — the launch flags are
# recorded with the leg, never assumed here); the frozen prompt bytes come
# from the ONE home (p0-bench-llama.sh via extract-frozen-p2-prompt.sh and
# the SYS/P1 sed extraction — never a second copy to drift); request bodies
# are exactly the p0 suite's shape (system+user, temperature 0.0, non-stream).
#
# SPEC grammar: CLASS:CAP[,CLASS:CAP...] — every pair runs REPS sequential
# reps, in spec order.
#   P1      — the frozen P1 short class (the p0 suite's own bytes)
#   P2      — the frozen P2 agent-turn class (the review prompt)
#   CAL<n>  — the frozen P2 class with the study padding marker plus exactly
#             n " pad" fillers appended (n=0 is the calibration shape; the
#             achieved prompt residue is reported from usage.prompt_tokens)
#
# Per-request evidence: req json, resp json, wall timing, server-log slice.
# Per-leg evidence: summary TSV (field-aware probes downstream — awk -F'\t',
# never a tab-collapsed display) + RAM/VRAM samples + the leg meta.
#
# Usage: determinism-study-leg.sh ENDPOINT SERVER_PID SERVER_LOG OUT_DIR LABEL REPS SPEC
set -euo pipefail

if [ "$#" -ne 7 ]; then
    echo "usage: $0 ENDPOINT SERVER_PID SERVER_LOG OUT_DIR LABEL REPS SPEC" >&2
    exit 2
fi

endpoint=$1
server_pid=$2
server_log=$3
out=$4
label=$5
reps=$6
spec=$7

mkdir -p "$out"

SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# The frozen bytes — the ONE home, fail-closed (the longleg precedent).
SYS=$(sed -n 's/^SYS="\(.*\)"$/\1/p' "$SCRIPTS_DIR/p0-bench-llama.sh" | head -1)
if [ -z "${SYS:-}" ]; then
    echo "FATAL: the p0 suite's SYS line not found in $SCRIPTS_DIR/p0-bench-llama.sh — the frozen suite changed; re-freeze explicitly, never substitute" >&2
    exit 3
fi
P1=$(sed -n 's/^P1="\(.*\)"$/\1/p' "$SCRIPTS_DIR/p0-bench-llama.sh" | head -1)
if [ -z "${P1:-}" ]; then
    echo "FATAL: the p0 suite's P1 line not found in $SCRIPTS_DIR/p0-bench-llama.sh — the frozen suite changed; re-freeze explicitly, never substitute" >&2
    exit 3
fi
P2_FULL=$(bash "$SCRIPTS_DIR/extract-frozen-p2-prompt.sh") || {
    echo "FATAL: the frozen P2 prompt could not be extracted — do not substitute bytes" >&2
    exit 3
}

json_escape() {
    local s=$1
    s=${s//\\/\\\\}
    s=${s//\"/\\\"}
    s=${s//$'\n'/\\n}
    s=${s//$'\t'/\\t}
    s=${s//$'\r'/\\r}
    printf '%s' "$s"
}

# The study padding: a marker sentence (the task stays semantically intact)
# plus n " pad" fillers. The marker's token cost is a constant shift — the
# ACHIEVED residue (usage.prompt_tokens mod n_ubatch) is the honest
# observable; the target is nominal only.
padded_p2() { # NFILL -> the padded prompt bytes on stdout
    local n=$1 fillers
    if [ "$n" -gt 0 ]; then
        fillers=$(printf ' pad%.0s' $(seq 1 "$n"))
        printf '%s\n\n[Study padding follows; it carries no information about the module. Ignore it when composing your review.]\n%s\n' "$P2_FULL" "$fillers"
    else
        printf '%s\n\n[Study padding follows; it carries no information about the module. Ignore it when composing your review.]\n' "$P2_FULL"
    fi
}

build_request() { # PROMPT MAX_TOKENS OUTFILE
    printf '{"messages":[{"role":"system","content":"%s"},{"role":"user","content":"%s"}],"temperature":0.0,"max_tokens":%s,"stream":false}' \
        "$(json_escape "$SYS")" "$(json_escape "$1")" "$2" \
        > "$3"
}

sample_mem() { # WHAT OUTFILE
    {
        echo "# $1 $(date -Is)"
        if [ -r "/proc/$server_pid/status" ]; then
            echo "vm_rss_kb=$(awk '/^VmRSS:/ {print $2}' "/proc/$server_pid/status")"
            echo "vm_hwm_kb=$(awk '/^VmHWM:/ {print $2}' "/proc/$server_pid/status")"
        else
            echo "vm_rss_kb=unavailable"
        fi
        echo "mem_available_kb=$(awk '/MemAvailable:/ {print $2}' /proc/meminfo)"
        for f in /sys/class/drm/card*/device/mem_info_vram_used; do
            [ -r "$f" ] && echo "$(basename "$(dirname "$(dirname "$f")")")_vram_used_bytes=$(cat "$f")"
        done
    } >> "$2"
}

server_log_size() { stat -c %s "$server_log" 2>/dev/null || echo 0; }

summary="$out/${label}_summary.tsv"
if [ ! -f "$summary" ]; then
    printf 'label\tclass\tcap\trep\ttotal_s\tprompt_tokens\tcompletion_tokens\tfinish_reason\treq_json\n' > "$summary"
fi
mem="$out/${label}_mem.txt"
sample_mem leg_start "$mem"

echo "# leg label=$label start $(date -Is) endpoint=$endpoint spec=$spec reps=$reps" > "$out/${label}.meta"

IFS=',' read -r -a pairs <<< "$spec"
for pair in "${pairs[@]}"; do
    class=${pair%%:*}
    cap=${pair##*:}
    case "$class" in
        P1) prompt=$P1 ;;
        P2) prompt=$P2_FULL ;;
        CAL*) prompt=$(padded_p2 "${class#CAL}") ;;
        *)
            echo "FATAL: unknown class $class in spec — the grammar is P1|P2|CAL<n>" >&2
            exit 3
            ;;
    esac

    for rep in $(seq 1 "$reps"); do
        req="$out/${label}_${class}_cap${cap}_r${rep}_req.json"
        resp="$out/${label}_${class}_cap${cap}_r${rep}_resp.json"
        timing="$out/${label}_${class}_cap${cap}_r${rep}.timing"
        build_request "$prompt" "$cap" "$req"
        log_before=$(server_log_size)
        t0=$(date +%s.%N)
        curl -sS --max-time 1800 "$endpoint/v1/chat/completions" \
            -H 'Content-Type: application/json' --data-binary "@$req" > "$resp"
        t1=$(date +%s.%N)
        awk -v a="$t0" -v b="$t1" 'BEGIN {printf "%.3f\n", b-a}' > "$timing"
        sleep 0.3
        tail -c +$((log_before + 1)) "$server_log" \
            > "$out/${label}_${class}_cap${cap}_r${rep}_srvlog.txt" 2>/dev/null || true
        ptok=$(jq -r '.usage.prompt_tokens // empty' "$resp" 2>/dev/null || true)
        ctok=$(jq -r '.usage.completion_tokens // empty' "$resp" 2>/dev/null || true)
        fin=$(jq -r '.choices[0].finish_reason // empty' "$resp" 2>/dev/null || true)
        if [ -z "$ptok" ] || [ -z "$fin" ]; then
            echo "FATAL: rep $resp did not answer a parseable completion (prompt_tokens/finish_reason empty) — protocol deviation, stop" >&2
            exit 6
        fi
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$label" "$class" "$cap" "$rep" "$(cat "$timing")" \
            "$ptok" "$ctok" "$fin" "${req##*/}" >> "$summary"
    done
done

sample_mem leg_end "$mem"
echo "# done $(date -Is)" >> "$out/${label}.meta"
echo "LEG_DONE $label -> $summary"
