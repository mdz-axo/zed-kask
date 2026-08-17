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
# Concurrency: cases within a model run in parallel via `xargs -P`. Each case
# is timed independently (own `date +%s%N` clock + own ttft probe), so tok/s
# and TTFT are correct under parallelism. Results land in per-case files
# (RESULTS_DIR/<idx>.line) and are concatenated in idx order for a deterministic
# summary regardless of completion order. Set CONCURRENCY (default 8) to tune.
#
# Usage:
#   check-classifier-models.sh [model-id ...]            # eval + per-model summary
#   check-classifier-models.sh --worker RDIR TOTAL ROW   # one case (internal)
# Requires: curl, jq, flock, base64, xargs. Reads OPENROUTER_API_KEY from kask/.env.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
EVAL_SET="$REPO_ROOT/kask/docs/review/eval_set.json"

# --- worker mode: run exactly one case -------------------------------------
# Args: RESULTS_DIR TOTAL ROW , where ROW = idx|id|task|gold|text_b64
if [ "${1:-}" = "--worker" ]; then
  RESULTS_DIR="$2"; TOTAL="$3"; ROW="$4"
  KEY="$(grep '^OPENROUTER_API_KEY=' "$REPO_ROOT/kask/.env" | tail -1 | cut -d= -f2- | tr -d '\"'"'")"
  # Row = idx|id|task|gold|text_b64|model  (6 fields; model appended by dispatcher)
  IFS='|' read -r idx id task gold text_b64 model <<< "$ROW"
  text="$(printf '%s' "$text_b64" | base64 -d)"
  case "$id" in S01|S02|S03) tag=INCTX ;; *) tag=SCORED ;; esac

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

  outfile="$(mktemp)"; ttft_file="$(mktemp)"
  t0="$(date +%s%N)"
  status=ERR; pred="-"; ttft=0; total_ms=0; tok=0
  if curl -sS --max-time 120 "https://openrouter.ai/api/v1/chat/completions" \
      -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
      -H "X-Title: classifier-eval" -N --data "$body" 2>/dev/null \
      | awk -v tf="$ttft_file" '/"content"/ && !done { ("date +%s%N" | getline now); close("date +%s%N"); print now > tf; done=1 } { print }' \
      > "$outfile"; then
    t1="$(date +%s%N)"
    total_ms=$(( (t1 - t0) / 1000000 ))
    [ -s "$ttft_file" ] && ttft=$(( ("$(cat "$ttft_file")" - t0) / 1000000 ))
    field="$(field_for_task "$task")"
    pred="$(grep '^data: ' "$outfile" | sed 's/^data: //' | grep -v '^\[DONE\]$' \
      | jq -rs --arg f "$field" '
          [ .[] | (.choices // [])[] | (.delta.content // "") ] | join("")
          | capture("\"" + $f + "\"\\s*:\\s*\"(?<lbl>[^\"]+)\""; "i") | .lbl // empty
        ' 2>/dev/null | head -1)"
    tok="$(grep '^data: ' "$outfile" | sed 's/^data: //' | grep -v '^\[DONE\]$' \
      | jq -rs '[ .[] | (.usage.completion_tokens // empty) ] | last // 0' 2>/dev/null)"
    if [ -z "$pred" ]; then status=NOPARSE; pred="-";
    elif [ "$(printf '%s' "$pred" | tr '[:upper:]' '[:lower:]')" = "$(printf '%s' "$gold" | tr '[:upper:]' '[:lower:]')" ]; then status=OK;
    else status=BAD; fi
  else
    status=ERR; pred=TRANSPORT
  fi
  rm -f "$outfile" "$ttft_file"
  # Result line: model|id|task|gold|status|pred|ttft|total_ms|tok  (deterministic per idx)
  printf '%s|%s|%s|%s|%s|%s|%s|%s|%s\n' "$model" "$id" "$task" "$gold" "$status" "$pred" "$ttft" "$total_ms" "$tok" > "$RESULTS_DIR/$idx.line"

  # Running completed-count under a lock so progress is quantifiable under concurrency.
  cnt_lock="$RESULTS_DIR/.counter.lock"
  exec 9>"$cnt_lock"
  flock 9
  n="$(cat "$RESULTS_DIR/.counter" 2>/dev/null || echo 0)"
  n=$((n+1)); echo "$n" > "$RESULTS_DIR/.counter"
  flock -u 9; exec 9>&-
  printf '%s [%s %d/%d] %s gold=%s pred=%s\n' "$id" "$tag" "$n" "$TOTAL" "$status" "$gold" "$pred" >&2
  exit 0
fi

# --- top-level: eval + summary --------------------------------------------
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

CONCURRENCY="${CONCURRENCY:-8}"
N_CASES="$(jq '.cases | length' "$EVAL_SET")"
SCRIPT="$REPO_ROOT/kask/scripts/check-classifier-models.sh"

echo "model|correct/47|inctx/3|section/17|dimension/20|failure/10|ttft_p50_ms|tok_s_p50"
for model in "${MODELS[@]}"; do
  echo "== $model ($N_CASES cases, concurrency $CONCURRENCY) ==" >&2
  RDIR="$(mktemp -d)"; : > "$RDIR/.counter"
  # Build one dispatch row per case: idx|id|task|gold|text_b64|model
  rows="$(mktemp)"
  for i in $(seq 0 $((N_CASES - 1))); do
    row="$(jq -c --argjson i "$i" '.cases[$i]' "$EVAL_SET")"
    id="$(echo "$row" | jq -r '.id')"; task="$(echo "$row" | jq -r '.task')"
    gold="$(echo "$row" | jq -r '.gold')"; text="$(echo "$row" | jq -r '.text')"
    tb="$(printf '%s' "$text" | base64 -w0)"
    printf '%s|%s|%s|%s|%s|%s\n' "$i" "$id" "$task" "$gold" "$tb" "$model" >> "$rows"
  done
  # Fan out. Workers never exit non-zero (they always write a result line),
  # but guard so one transport failure can't abort the batch.
  xargs -P "$CONCURRENCY" -a "$rows" -I {} \
    bash "$SCRIPT" --worker "$RDIR" "$N_CASES" {} >/dev/null || true
  rm -f "$rows"

  # Collect in idx order (deterministic regardless of completion order).
  model_res="$(mktemp)"
  for i in $(seq 0 $((N_CASES - 1))); do
    [ -f "$RDIR/$i.line" ] && cat "$RDIR/$i.line" >> "$model_res" \
      || printf '%s|idx%s|?|ERR|MISSING|0|0|0\n' "$model" "$i" >> "$model_res"
  done

  # Result line fields: 1=model 2=id 3=task 4=gold 5=status 6=pred 7=ttft 8=total_ms 9=tok.
  # In-context examples S01-S03 are excluded from the accuracy total and from
  # per-task scored counts (reported only in the inctx/3 column).
  ok="$(awk -F'|' '$2 !~ /^S0[123]$/ && $5 == "OK"' "$model_res" | wc -l)"
  scored="$(awk -F'|' '$2 !~ /^S0[123]$/' "$model_res" | wc -l)"
  inctx="$(awk -F'|' '$2 ~ /^S0[123]$/ && $5 == "OK"' "$model_res" | wc -l)"
  sec="$(awk -F'|' '$3 == "section" && $2 !~ /^S0[123]$/ && $5 == "OK"' "$model_res" | wc -l)"
  sec_t="$(awk -F'|' '$3 == "section" && $2 !~ /^S0[123]$/' "$model_res" | wc -l)"
  dim="$(awk -F'|' '$3 == "dimension" && $5 == "OK"' "$model_res" | wc -l)"
  dim_t="$(awk -F'|' '$3 == "dimension"' "$model_res" | wc -l)"
  fail="$(awk -F'|' '$3 == "failure" && $5 == "OK"' "$model_res" | wc -l)"
  fail_t="$(awk -F'|' '$3 == "failure"' "$model_res" | wc -l)"
  ttft="$(awk -F'|' '$7 != "0" && $7 != "" {print $7}' "$model_res" | sort -n | awk '
    { a[NR]=$1 } END { if (NR == 0) { print "n/a" } else if (NR % 2 == 1) { print a[(NR+1)/2] } else { print (a[NR/2]+a[NR/2+1])/2 } }')"
  # tok/s over the generation window (total - ttft), median across cases.
  tok_s="$(awk -F'|' '$9 > 0 && ($8 - $7) > 0 { printf "%.1f\n", $9 / (($8 - $7) / 1000.0) }' "$model_res" | sort -n | awk '
    { a[NR]=$1 } END { if (NR == 0) { print "n/a" } else if (NR % 2 == 1) { print a[(NR+1)/2] } else { print (a[NR/2]+a[NR/2+1])/2 } }')"
  echo "$model|$ok/$scored|$inctx/3|$sec/$sec_t|$dim/$dim_t|$fail/$fail_t|$ttft|$tok_s"
  rm -rf "$RDIR" "$model_res"
done