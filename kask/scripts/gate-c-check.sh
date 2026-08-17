#!/usr/bin/env bash
# Gate (c) live check: send reasoning.enabled=false to each model on one probe
# passage; classify the endpoint response as PASS (served non-thinking), FAIL
# (reasoning mandatory / 400), or ERROR (transport). Prints a table.
# Usage: gate-c-check.sh model-id [model-id ...]
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
KEY="$(grep '^OPENROUTER_API_KEY=' "$REPO_ROOT/kask/.env" | tail -1 | cut -d= -f2- | tr -d '\"'"'")"
[ -n "$KEY" ] || { echo "no key" >&2; exit 1; }

PROBE='Passage: "Every inference request must carry an explicit capability token."'
SYS='Classify this passage. Return ONLY: {"category":"X"} where X is Statement, Evidence, Diagram, or Implications.'

echo "model|gate_c|http|note"
for model in "$@"; do
  body="$(jq -n --arg m "$model" --arg s "$SYS" --arg p "$PROBE" '{
    model:$m,
    messages:[{role:"system",content:$s},{role:"user",content:$p}],
    temperature:0.0, max_tokens:60, stream:false,
    reasoning:{enabled:false}
  }')"
  resp="$(curl -sS --max-time 60 -w '\n%{http_code}' \
    "https://openrouter.ai/api/v1/chat/completions" \
    -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
    -H "X-Title: gate-c-check" --data "$body" 2>/dev/null || true)"
  code="$(echo "$resp" | tail -1)"
  bodyr="$(echo "$resp" | sed '$d')"
  if [ "$code" = "200" ]; then
    # check whether the endpoint complained reasoning mandatory inside a 200 error envelope
    if echo "$bodyr" | grep -qi "reasoning is mandatory"; then
      echo "$model|FAIL|$code|reasoning mandatory"
    else
      content="$(echo "$bodyr" | jq -r '.choices[0].message.content // empty' 2>/dev/null | head -c 60)"
      echo "$model|PASS|$code|served: ${content}"
    fi
  elif echo "$bodyr" | grep -qi "reasoning is mandatory"; then
    echo "$model|FAIL|$code|reasoning mandatory"
  else
    msg="$(echo "$bodyr" | jq -r '.error.message // empty' 2>/dev/null | head -c 80)"
    echo "$model|ERROR|$code|${msg:-transport}"
  fi
  sleep 0.3
done
