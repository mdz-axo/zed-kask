#!/usr/bin/env bash
# Falsifier probe for the event-substrate proposal (risk 5).
#
# Question: do eval tasks share structure across agent cards, or does the
# harness collapse into per-agent bespoke testing?
#
# Method: replicate AgentExecutor's exact prompt shape
# ("{system_prompt}\n\n---\n\nTask: {task}") and run_evaluator's three
# evaluator kinds (contains / not_contains / regex) against the local
# Ollama backend, 3 agents x 3 tasks x 3 repeats. Compute the same
# pass-rate and standard-error math as eval_task_report in local_tools.rs.
#
# Tasks are chosen so string-match is a VALID oracle (review finding 1):
# each task's success criterion is a substring the agent was explicitly
# instructed to emit, not a semantic judgment.
#
# A crashed rollout (curl/jq failure) counts as a failed rollout — the
# pass rate measures end-to-end reliability, which includes crashes.

set -euo pipefail

OLLAMA="${OLLAMA:-http://localhost:11434/api/chat}"
MODEL="${MODEL:-qwen3.8:27b}"
REPEATS="${REPEATS:-3}"

for bin in curl jq awk; do
    command -v "$bin" >/dev/null 2>&1 || { echo "missing dependency: $bin" >&2; exit 1; }
done

# run_evaluator's three kinds, in bash:
#   contains     -> grep -qF (fixed-string substring, like Rust str::contains)
#   not_contains -> negated grep -qF
#   regex        -> grep -qE
evaluate() {
    local response="$1" kind="$2" spec="$3"
    case "$kind" in
        contains)     grep -qF -- "$spec" <<<"$response" ;;
        not_contains) ! grep -qF -- "$spec" <<<"$response" ;;
        regex)        grep -qE -- "$spec" <<<"$response" ;;
        *)            echo "unknown evaluator kind: $kind" >&2; return 2 ;;
    esac
}

# One rollout: build the AgentExecutor prompt shape, call Ollama, return the
# response text on stdout. Any failure propagates to the caller (counted as
# an error rollout there).
run_once() {
    local system_prompt="$1" task="$2"
    local payload response
    payload=$(jq -n --arg model "$MODEL" --arg prompt "$system_prompt

---

Task: $task" \
        '{model: $model, messages: [{role: "user", content: $prompt}], stream: false}')
    response=$(curl -s --max-time 300 -H 'Content-Type: application/json' \
        -d "$payload" "$OLLAMA")
    jq -r '.message.content' <<<"$response"
}

NARRATOR_PROMPT='You are a local narrator. Given a structured input (JSON or prose), produce a concise narrative summary of 3-5 sentences. Stay faithful to the input - do not invent facts. If the input is empty or unintelligible, say so plainly. Keep the summary self-contained; do not reference this prompt.'
EXTRACTOR_PROMPT='You are a local extractor. Given unstructured text, extract the requested fields and report each on its own line as '\''field: value'\''. Stay faithful to the input - do not invent values. If a requested field is absent, report '\''field: not found'\''. Keep the output self-contained; do not reference this prompt.'
CRITIC_PROMPT='You are a local critic. Given a text and review criteria, judge the text against each criterion and answer each question with '\''yes'\'' or '\''no'\'' plus a one-line reason, one per line. Stay faithful to the input - judge only what is asked. Keep the output self-contained; do not reference this prompt.'

# Task structure is IDENTICAL across agents: (task, evaluator kind, spec),
# separated by ASCII unit separators so task text may contain | and :.
# Only the payload differs — this is the sharing the probe measures.
declare -A AGENTS=(
    [local_narrator]="$NARRATOR_PROMPT"
    [local_extractor]="$EXTRACTOR_PROMPT"
    [local_critic]="$CRITIC_PROMPT"
)
declare -A TASKS=(
    [local_narrator]=$'Summarize this input: The quarterly report shows revenue of $4.2M, up 12% year over year, with churn flat at 3%. Answer in 3-5 sentences.\x1fcontains\x1frevenue\nSummarize this input: {"event": "deploy", "service": "api-gateway", "status": "healthy"}. Answer in 3-5 sentences.\x1fcontains\x1fapi-gateway\nSummarize this input: empty. Answer in 3-5 sentences.\x1fnot_contains\x1finvented fact'
    [local_extractor]=$'From this text, extract the fields "revenue", "growth", "churn": The quarterly report shows revenue of $4.2M, up 12% year over year, with churn flat at 3%.\x1fcontains\x1frevenue\nFrom this text, extract the fields "event", "service", "status": {"event": "deploy", "service": "api-gateway", "status": "healthy"}\x1fcontains\x1fapi-gateway\nFrom this text, extract the fields "revenue", "growth": The meeting notes mention no financial figures.\x1fnot_contains\x1finvented fact'
    [local_critic]=$'Review this text against the criteria "mentions revenue" and "mentions growth": The quarterly report shows revenue of $4.2M, up 12% year over year.\x1fcontains\x1frevenue\nReview this text against the criteria "names the service" and "states the status": {"event": "deploy", "service": "api-gateway", "status": "healthy"}\x1fcontains\x1fapi-gateway\nReview this text against the criteria "mentions revenue": The meeting notes mention no financial figures.\x1fnot_contains\x1finvented fact'
)

grand_passes=0
grand_total=0

for agent in local_narrator local_extractor local_critic; do
    echo
    echo "=== $agent ==="
    system_prompt="${AGENTS[$agent]}"
    while IFS= read -r entry; do
        task="${entry%%$'\x1f'*}"
        rest="${entry#*$'\x1f'}"
        kind="${rest%%$'\x1f'*}"
        spec="${rest#*$'\x1f'}"

        passes=0
        errors=0
        for _ in $(seq 1 "$REPEATS"); do
            if response=$(run_once "$system_prompt" "$task") \
                && evaluate "$response" "$kind" "$spec"; then
                passes=$((passes + 1))
            else
                errors=$((errors + 1))
            fi
        done

        attempts=$((passes + errors))
        grand_passes=$((grand_passes + passes))
        grand_total=$((grand_total + attempts))

        # Same math as eval_task_report: p = passes/attempts,
        # se = sqrt(p(1-p)/n); se is undefined for n <= 1 (printed as nan,
        # never 0 — one observation supports no certainty).
        stats=$(awk -v p="$passes" -v n="$attempts" 'BEGIN {
            rate = (n > 0) ? p / n : nan
            se = (n > 1) ? sqrt(rate * (1 - rate) / n) : nan
            printf "pass_rate=%.3f std_error=%.3f", rate, se
        }')
        printf '  task=%s... eval=%s:%s\n' \
            "${task:0:60}" "$kind" "'$spec'"
        printf '    passes=%d/%d errors=%d %s\n' \
            "$passes" "$attempts" "$errors" "$stats"
    done <<<"${TASKS[$agent]}"
done

echo
awk -v p="$grand_passes" -v n="$grand_total" 'BEGIN {
    printf "=== overall: %d/%d = %.3f ===\n", p, n, (n > 0) ? p / n : nan
}'
