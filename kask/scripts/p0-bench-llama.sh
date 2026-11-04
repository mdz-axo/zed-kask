#!/usr/bin/env bash
# p0-bench-llama.sh — the P0 fixed private-workload prompt suite + measurement
# harness for the private-inference local leg (work order 2026-11-04, phase 4).
#
# The server is launched BY HAND per leg (P0 discipline: the canonical GPU leg
# launches through the placement compiler's rendered LaunchPlan); this script
# only measures an ALREADY-READY llama-server and records raw evidence:
#
#   - per-request curl timings: TTFB + total (non-stream), and for the
#     streaming rep TTFB is recorded as the TTFT candidate — validated at the
#     first leg against the known prefill duration (llama-server flushes SSE
#     headers with the first token; if a leg shows near-zero TTFB on the
#     long-prefill prompt, TTFT must be re-taken with a first-content-line
#     timer and this script patched — the validation is part of the method)
#   - per-request server-log slices (the server's own prompt-eval/eval report)
#   - response bodies (usage token counts) for every request
#   - post-load and post-suite RAM (VmRSS/VmHWM) + amdgpu VRAM samples
#
# Usage: p0-bench-llama.sh ENDPOINT SERVER_PID SERVER_LOG OUT_DIR LABEL
#   ENDPOINT    e.g. http://127.0.0.1:8010   (llama-server OpenAI API root)
#   SERVER_PID  pid of the running llama-server (for /proc RAM sampling)
#   SERVER_LOG  the server's log file, ABSOLUTE path (sliced per request)
#   OUT_DIR     destination for this leg's raw artifacts
#   LABEL       e.g. cpu-control | vulkan-fa-on | vulkan-fa-off | hip-fa-on
#
# The fixed suite (byte-frozen 2026-11-04): 3 prompt classes x 3 non-stream
# reps + 1 stream rep each, temperature 0 (greedy), max_tokens 128/256/256,
# one discarded warmup. Code excerpts in P2/P3 are frozen copies of
# kask/crates/hkask-private-inference files at the 2026-11-04 handoff tree.
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

# ---------------------------------------------------------------------------
# JSON escaping for prompt bodies (backslash first, then quote, then control
# chars). The frozen excerpts carry Rust escape sequences — this must be exact.
# ---------------------------------------------------------------------------
json_escape() {
    local s=$1
    s=${s//\\/\\\\}
    s=${s//\"/\\\"}
    s=${s//$'\n'/\\n}
    s=${s//$'\t'/\\t}
    s=${s//$'\r'/\\r}
    printf '%s' "$s"
}

# build_request PROMPT MAX_TOKENS STREAM OUTFILE
build_request() {
    local prompt=$1 max_tokens=$2 stream=$3 outfile=$4
    printf '{"messages":[{"role":"system","content":"%s"},{"role":"user","content":"%s"}],"temperature":0.0,"max_tokens":%s,"stream":%s}' \
        "$(json_escape "$SYS")" "$(json_escape "$prompt")" "$max_tokens" "$stream" \
        > "$outfile"
}

# ---------------------------------------------------------------------------
# The fixed private-workload prompt suite (byte-frozen; the P0 baseline record
# §4 carries the definition and every deviation).
# ---------------------------------------------------------------------------
SYS="You are a senior Rust systems engineer working inside a private, local-only assistant environment. Answer with precision and brevity. When reviewing code, name concrete, actionable improvements; never speculate about code you cannot see."

P1="Explain in two sentences when a Rust enum is preferable to a struct with a discriminator field, and name the one cost that decides it."

P2=$(cat <<'PROMPT_EOF'
Review the following Rust module (the LaunchPlan renderer of a placement compiler) and list exactly three concrete improvements with a one-line rationale each.

--- BEGIN MODULE (launch.rs) ---
//! The LaunchPlan — the placement compiler's output and the future
//! supervisor's input (L4). The plan carries concrete flag spellings
//! resolved through the manifest vocabulary; `Display` renders the
//! command line. Emission stays typed Rust in v1 — the jinja
//! unification seam (cloud-leg deploy templates) is the named promotion
//! trigger, not a day-one layer (plan §10, delta 4).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub surface_id: String,
    pub binary: String,
    pub host: String,
    pub port: u16,
    pub args: Vec<String>,
}

impl LaunchPlan {
    /// The loopback endpoint the plan serves — the supervisor's health
    /// probe and the future `openai_compatible` settings entry both use
    /// it.
    pub fn endpoint(&self) -> String {
        format!("http://{}:{}/v1", self.host, self.port)
    }
}

impl fmt::Display for LaunchPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.binary)?;
        for arg in &self.args {
            write!(f, " {}", shell_quote(arg))?;
        }
        Ok(())
    }
}

/// Minimal shell quoting: wrap in single quotes when the argument carries
/// anything the shell would interpret.
fn shell_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "''".into();
    }
    let safe = arg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_' | '=' | ':'));
    if safe {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}
--- END MODULE (launch.rs) ---
PROMPT_EOF
)

P3=$(cat <<'PROMPT_EOF'
Summarize the guard semantics of the following Rust module excerpt (a constraint evaluator for GPU-launch plans), then name the two seams you consider most fragile and why.

--- BEGIN MODULE EXCERPT (manifest.rs, evaluation arms) ---
/// Evaluate one constraint against an env. Deterministic; the arithmetic
/// lives here, the parameters live in the manifest data (plan §4).
pub fn evaluate_constraint(
    constraint: &Constraint,
    env: &ConstraintEnv,
) -> Result<Evaluation, ManifestError> {
    match constraint {
        Constraint::AvailableRamHeadroom { headroom_factor } => {
            let name = "available_ram_headroom";
            let anonymous = env
                .kv_bytes_estimate
                .checked_add(env.compute_bytes_estimate)
                .ok_or_else(|| ManifestError::Constraint {
                    constraint: name.into(),
                    reason: "anonymous byte sum overflowed u64".into(),
                })?;
            let resident = if env.model_mmap {
                anonymous
            } else {
                anonymous
                    .checked_add(env.expert_bytes)
                    .and_then(|b| b.checked_add(env.dense_bytes))
                    .ok_or_else(|| ManifestError::Constraint {
                        constraint: name.into(),
                        reason: "resident byte sum overflowed u64".into(),
                    })?
            };
            let budget = (env.available_ram_bytes as f64 * (1.0 - headroom_factor)) as u64;
            if resident <= budget {
                Ok(Evaluation::Allow)
            } else {
                Ok(Evaluation::Deny(format!(
                    "resident {resident} bytes exceeds budget {budget} \
                     (available {} x (1 - {headroom_factor}))",
                    env.available_ram_bytes
                )))
            }
        }
        Constraint::VramDenseFit => {
            if !env.gpu_offload {
                return Ok(Evaluation::Allow);
            }
            let Some(vram) = env.vram_bytes_declared else {
                return Ok(Evaluation::Deny(
                    "GPU offload planned but no VRAM budget declared".into(),
                ));
            };
            let resident = env
                .dense_bytes
                .checked_add(env.kv_bytes_estimate)
                .ok_or_else(|| ManifestError::Constraint {
                    constraint: "vram_dense_fit".into(),
                    reason: "dense+kv byte sum overflowed u64".into(),
                })?;
            if resident <= vram {
                Ok(Evaluation::Allow)
            } else {
                Ok(Evaluation::Deny(format!(
                    "dense {dense} + kv {kv} bytes exceed the declared VRAM budget {vram}",
                    dense = env.dense_bytes,
                    kv = env.kv_bytes_estimate
                )))
            }
        }
        Constraint::ExpertSlotFloor { floor_mult } => match (env.slot_count, env.n_expert_used) {
            (None, None) => Ok(Evaluation::Allow),
            (Some(slots), Some(used)) => {
                let floor = floor_mult.saturating_mul(used);
                if slots >= floor {
                    Ok(Evaluation::Allow)
                } else {
                    Ok(Evaluation::Deny(format!(
                        "expert cache slots {slots} below floor {floor_mult}x{used} = {floor}"
                    )))
                }
            }
            _ => Ok(Evaluation::Deny(
                "streaming plan is half-declared: slot_count and n_expert_used must both be set"
                    .into(),
            )),
        },
    }
}
--- END MODULE EXCERPT (manifest.rs) ---
PROMPT_EOF
)

# prompt_id:max_tokens — P1 short, P2 agent-turn, P3 long-context.
SUITE=("P1:128" "P2:256" "P3:256")
NONSTREAM_REPS=3

# ---------------------------------------------------------------------------
# Samplers
# ---------------------------------------------------------------------------
sample_mem() {
    # sample_mem WHAT OUTFILE — server RSS/HWM, system MemAvailable, per-card
    # amdgpu VRAM-in-use.
    local what=$1 outfile=$2
    {
        echo "# $what $(date -Is)"
        if [ -r "/proc/$server_pid/status" ]; then
            echo "vm_rss_kb=$(awk '/^VmRSS:/ {print $2}' "/proc/$server_pid/status")"
            echo "vm_hwm_kb=$(awk '/^VmHWM:/ {print $2}' "/proc/$server_pid/status")"
        else
            echo "vm_rss_kb=unavailable"
            echo "vm_hwm_kb=unavailable"
        fi
        echo "mem_available_kb=$(awk '/^MemAvailable:/ {print $2}' /proc/meminfo)"
        for card in /sys/class/drm/card*/device/mem_info_vram_used; do
            [ -f "$card" ] || continue
            echo "vram_used[$card]=$(cat "$card")"
        done
    } >> "$outfile"
}

server_log_size() {
    stat -c %s "$server_log" 2>/dev/null || echo 0
}

# usage_of BODYFILE TOKEN_KEY -> prints the integer token count (blank on miss)
usage_of() {
    grep -o "\"$2\": *[0-9]*" "$1" 2>/dev/null | head -1 | grep -o '[0-9]*' || true
}

# ---------------------------------------------------------------------------
# Run the suite
# ---------------------------------------------------------------------------
summary="$out/${label}_summary.tsv"
printf 'label\tprompt_id\trep\tmode\tttfb_s\tttft_s\ttotal_s\tprompt_tokens\tcompletion_tokens\treq_json\n' > "$summary"
mem="$out/${label}_mem.txt"; : > "$mem"

sample_mem post_load "$mem"

# Warmup (discarded): one full non-stream P2 pass.
build_request "$P2" 256 false "$out/${label}_warmup_req.json"
curl -s -o /dev/null -X POST "$endpoint/v1/chat/completions" \
    -H 'Content-Type: application/json' \
    --data-binary "@$out/${label}_warmup_req.json"
sleep 1

for entry in "${SUITE[@]}"; do
    prompt_id=${entry%%:*}
    max_tokens=${entry##*:}
    prompt_var="$prompt_id"
    prompt=${!prompt_var}

    for rep in $(seq 1 "$NONSTREAM_REPS"); do
        mode=nonstream
        req="$out/${label}_${prompt_id}_${mode}_${rep}_req.json"
        resp="$out/${label}_${prompt_id}_${mode}_${rep}_resp.json"
        build_request "$prompt" "$max_tokens" false "$req"
        log_before=$(server_log_size)
        timings=$(curl -s -o "$resp" -X POST "$endpoint/v1/chat/completions" \
            -H 'Content-Type: application/json' \
            -w '%{time_starttransfer} %{time_total}' \
            --data-binary "@$req")
        sleep 0.3
        tail -c +$((log_before + 1)) "$server_log" \
            > "$out/${label}_${prompt_id}_${mode}_${rep}_srvlog.txt" 2>/dev/null || true
        ttfb=${timings%% *}
        total=${timings##* }
        ptok=$(usage_of "$resp" prompt_tokens)
        ctok=$(usage_of "$resp" completion_tokens)
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$label" "$prompt_id" "$rep" "$mode" "$ttfb" '' "$total" "$ptok" "$ctok" "${req##*/}" >> "$summary"
    done

    # One streaming rep per class: TTFB recorded as the TTFT candidate
    # (validated at the first leg — see the header comment).
    mode=stream
    rep=1
    req="$out/${label}_${prompt_id}_${mode}_${rep}_req.json"
    body="$out/${label}_${prompt_id}_${mode}_${rep}_body.txt"
    build_request "$prompt" "$max_tokens" true "$req"
    log_before=$(server_log_size)
    timings=$(curl -sN -o "$body" -X POST "$endpoint/v1/chat/completions" \
        -H 'Content-Type: application/json' \
        -w '%{time_starttransfer} %{time_total}' \
        --data-binary "@$req")
    sleep 0.3
    tail -c +$((log_before + 1)) "$server_log" \
        > "$out/${label}_${prompt_id}_${mode}_${rep}_srvlog.txt" 2>/dev/null || true
    ttfb=${timings%% *}
    total=${timings##* }
    ptok=$(usage_of "$body" prompt_tokens)
    ctok=$(usage_of "$body" completion_tokens)
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$label" "$prompt_id" "$rep" "$mode" "$ttfb" "$ttfb" "$total" "$ptok" "$ctok" "${req##*/}" >> "$summary"
done

sample_mem post_suite "$mem"

echo "BENCH_DONE $label -> $summary"
