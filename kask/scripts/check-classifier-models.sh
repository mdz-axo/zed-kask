#!/usr/bin/env bash
# Classifier-model evaluation against the real hKask label spaces.
#
# Runs each model over kask/docs/review/eval_set.json (50 observations on the
# three registry label spaces: 4-way section types from
# registry/classify/section-classifier.yaml, the 4-way Gentle/Schriver/
# Hopper/Lovelace dimension ontology from registry/classify/hmem-extractor.yaml,
# and the 6-way qa-triage failure types from registry/classify/qa-triage.yaml).
# Wire protocol matches the classifier: temperature 0.0, JSON-only output,
# reasoning disabled (hkask sends enable_thinking=false on the wire; see
# hkask-inference/src/chat_protocol.rs:109-114).
#
# S01-S03 are the section-classifier YAML's own 3-shot in-context examples and
# are reported separately, not counted in the accuracy total.
#
# Usage: check-classifier-models.sh [model-id ...]   (defaults below)
# Requires: curl, jq. Reads OPENROUTER_API_KEY from kask/.env.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
EVAL_SET="$REPO_ROOT/kask/docs/review/eval_set.json"

KEY="$(grep '^OPENROUTER_API_KEY=' "$REPO_ROOT/kask/.env" | tail -1 | cut -d= -f2- | tr -d '\"'"'")"
if [ -z "$KEY" ]; then
  echo "error: OPENROUTER_API_KEY not set in kask/.env" >&2
  exit 1
fi
[ -f "$EVAL_SET" ] || { echo "error: eval set not found: $EVAL_SET" >&2; exit 1; }

MODELS=("$@")
if [ ${#MODELS[@]} -eq 0 ]; then
  MODELS=(
    "deepseek/deepseek-v4-flash"
    "nvidia/nemotron-3.5-lightning"
    "nvidia/nemotron-3-super-120b-a12b"
    "z-ai/glm-5.2"
  )
fi

# Label space => (extract field, system prompt) from the registry manifests.
field_for_task() {
  case "$1" in
    section)   echo "category" ;;
    dimension) echo "primary_dimension" ;;
    failure)   echo "failure_type" ;;
  esac
}

system_for_task() {
  case "$1" in
    section) cat <<'EOF'
Classify this passage. Return ONLY: {"category":"X"}.

Categories:
Statement=principle/rule/assertion.
Evidence=example/data/citation (look for: "for instance", "for example").
Diagram=structure/layout/mechanical description.
Implications=consequence ("therefore", "thus", "hence").

Examples:

Passage: "OCAP delegation requires every capability access to carry an explicit, attenuating, unforgeable token."
{"category":"Statement"}

Passage: "For instance, a userpod calling version_info does not need access to the wallet — the capability grant is scoped to read-only visibility via the Regulation span registry."
{"category":"Evidence"}

Passage: "If Regulation thresholds are breached, the Curator escalates to the human operator. Therefore, alert fatigue must be managed through hysteresis and cooldown windows."
{"category":"Implications"}
EOF
    ;;
    dimension) cat <<'EOF'
Extract the primary dimension of this documentation passage. Return ONLY: {"primary_dimension":"X"}.

Primary dimension meanings:
- Gentle: agent-correctness, actionable, unambiguous
- Schriver: findability, scannable, well-structured
- Hopper: accessibility, comprehensible, plain language
- Lovelace: precision, verifiable, rigorous
EOF
    ;;
    failure) cat <<'EOF'
Diagnose this Rust test failure. Return ONLY: {"failure_type":"X"}.
failure_type must be one of: Panic | Assertion | Timeout | Flake | LogicError | MemoryError
EOF
    ;;
  esac
}

run_case() { # $1=model $2=task $3=gold $4=text ; writes RESULT_FILE line
  local model="$1" task="$2" gold="$3" text="$4"
  local sys body outfile t0 t1 ttft
  sys="$(system_for_task "$task")"
  body="$(jq -n --arg model "$model" --arg sys "$sys" --arg text "$text" '{
    model: $model,
    messages: [
      {role: "system", content: $sys},
      {role: "user", content: ("Passage: " + ($text | tojson))}
    ],
    temperature: 0.0, max_tokens: 150, stream: true,
    usage: {include: true}, reasoning: {enabled: false}
  }')"
  outfile="$(mktemp)"
  local ttft_file total_ms tok
  ttft_file="$(mktemp)"
  t0="$(date +%s%N)"
  if ! curl -sS --max-time 120 "https://openrouter.ai/api/v1/chat/completions" \
      -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
      -H "X-Title: classifier-eval" -N --data "$body" 2>/dev/null \
      | awk -v tf="$ttft_file" '/"content"/ && !done { ("date +%s%N" | getline now); close("date +%s%N"); print now > tf; done=1 } { print }' \
      > "$outfile"; then
    echo "$model|$task|$gold|ERR|TRANSPORT|0|0|0" >> "$RESULT_FILE"
    rm -f "$outfile" "$ttft_file"; return
  fi
  t1="$(date +%s%N)"
  total_ms=$(( (t1 - t0) / 1000000 ))
  if [ -s "$ttft_file" ]; then
    ttft=$(( ("$(cat "$ttft_file")" - t0) / 1000000 ))
  else
    ttft=0
  fi
  # SSE stream: collect content deltas, extract label from the first JSON field hit.
  local field pred
  field="$(field_for_task "$task")"
  pred="$(grep '^data: ' "$outfile" | sed 's/^data: //' | grep -v '^\[DONE\]$' \
    | jq -rs --arg f "$field" '
        [ .[] | (.choices // [])[] | (.delta.content // "") ] | join("")
        | capture("\"" + $f + "\"\\s*:\\s*\"(?<lbl>[^\"]+)\""; "i") | .lbl // empty
      ' 2>/dev/null | head -1)"
  tok="$(grep '^data: ' "$outfile" | sed 's/^data: //' | grep -v '^\[DONE\]$' \
    | jq -rs '[ .[] | (.usage.completion_tokens // empty) ] | last // 0' 2>/dev/null)"
  rm -f "$outfile" "$ttft_file"
  if [ -z "$pred" ]; then
    echo "$model|$task|$gold|NOPARSE|-|$ttft|$total_ms|$tok" >> "$RESULT_FILE"
  elif [ "$(printf '%s' "$pred" | tr '[:upper:]' '[:lower:]')" = "$(printf '%s' "$gold" | tr '[:upper:]' '[:lower:]')" ]; then
    echo "$model|$task|$gold|OK|$pred|$ttft|$total_ms|$tok" >> "$RESULT_FILE"
  else
    echo "$model|$task|$gold|BAD|$pred|$ttft|$total_ms|$tok" >> "$RESULT_FILE"
  fi
}

RESULT_FILE="$(mktemp)"
N_CASES="$(jq '.cases | length' "$EVAL_SET")"

for model in "${MODELS[@]}"; do
  echo "== $model ($N_CASES cases) ==" >&2
  for i in $(seq 0 $((N_CASES - 1))); do
    row="$(jq -c --argjson i "$i" '.cases[$i]' "$EVAL_SET")"
    id="$(echo "$row" | jq -r '.id')"; task="$(echo "$row" | jq -r '.task')"
    gold="$(echo "$row" | jq -r '.gold')"; text="$(echo "$row" | jq -r '.text')"
    case "$id" in S01|S02|S03) tag=INCTX ;; *) tag=SCORED ;; esac
    out="$(run_case "$model" "$task" "$gold" "$text"; tail -1 "$RESULT_FILE")"
    status="$(echo "$out" | cut -d'|' -f4)"
    echo "  $id [$tag] $status gold=$gold pred=$(echo "$out" | cut -d'|' -f5)" >&2
  done
done

echo ""
echo "model|correct/47|inctx/3|section/20|dimension/20|failure/10|ttft_p50_ms|tok_s_p50"
for model in "${MODELS[@]}"; do
  # scored = all rows excluding the 3 in-context examples (S01-S03 always run first)
  ok="$(grep "^$model|" "$RESULT_FILE" | tail -n +4 | awk -F'|' '$4 == "OK"' | wc -l)"
  scored="$(grep "^$model|" "$RESULT_FILE" | tail -n +4 | wc -l)"
  inctx="$(grep "^$model|" "$RESULT_FILE" | head -3 | awk -F'|' '$4 == "OK"' | wc -l)"
  sec="$(grep "^$model|section|" "$RESULT_FILE" | awk -F'|' '$4 == "OK"' | wc -l)"
  sec_t="$(grep -c "^$model|section|" "$RESULT_FILE" || true)"
  dim="$(grep "^$model|dimension|" "$RESULT_FILE" | awk -F'|' '$4 == "OK"' | wc -l)"
  dim_t="$(grep -c "^$model|dimension|" "$RESULT_FILE" || true)"
  fail="$(grep "^$model|failure|" "$RESULT_FILE" | awk -F'|' '$4 == "OK"' | wc -l)"
  fail_t="$(grep -c "^$model|failure|" "$RESULT_FILE" || true)"
  ttft="$(grep "^$model|" "$RESULT_FILE" | awk -F'|' '$6 != "0" {print $6}' | sort -n | awk '
    { a[NR]=$1 } END { if (NR == 0) { print "n/a" } else if (NR % 2 == 1) { print a[(NR+1)/2] } else { print (a[NR/2]+a[NR/2+1])/2 } }')"
  # tok/s over the generation window (total - ttft), median across cases
  tok_s="$(grep "^$model|" "$RESULT_FILE" | awk -F'|' '$8 > 0 && ($7 - $6) > 0 { printf "%.1f\n", $8 / (($7 - $6) / 1000.0) }' | sort -n | awk '
    { a[NR]=$1 } END { if (NR == 0) { print "n/a" } else if (NR % 2 == 1) { print a[(NR+1)/2] } else { print (a[NR/2]+a[NR/2+1])/2 } }')"
  echo "$model|$ok/$scored|$inctx/3|$sec/$sec_t|$dim/$dim_t|$fail/$fail_t|$ttft|$tok_s"
done

rm -f "$RESULT_FILE"
