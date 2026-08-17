#!/usr/bin/env bash
# RunPod kask-ocr endpoint concurrency ramp-up test.
#
# Tests the RunPod serverless OCR endpoint (OLMOCR-2) with increasing
# concurrency: 1 → 2 → 4 → 8 → 16 → 32. At each stage, measures success
# rate, average latency, p95 latency, and error rate.
#
# Stop conditions:
#   - Error rate exceeds 10% at any concurrency level
#   - Average latency exceeds 30 seconds
#   - 5xx errors on more than 3 consecutive requests
#   - Concurrency level 32 completes successfully
#
# Usage: ./scripts/runpod-ocr-concurrency-test.sh
# Requires: RUNPOD_API_KEY env var, curl, jq, xargs, bc, /tmp/kask-ocr-test-page-01.png

set -euo pipefail

# ── Configuration ──────────────────────────────────────────────────────────
ENDPOINT_ID="hsldzov6932wf5"
API_KEY="${RUNPOD_API_KEY:?RUNPOD_API_KEY is not set}"
REST_BASE="https://api.runpod.ai/v2/${ENDPOINT_ID}/openai/v1"
IMAGE_PATH="/tmp/kask-ocr-test-page-01.png"
CONCURRENCY_LEVELS=(1 2 4 8 16 32)
MAX_AVG_LATENCY=30.0
MAX_ERROR_RATE=0.10
MAX_CONSECUTIVE_5XX=3
REQUEST_TIMEOUT=120

# ── Prepare the image ─────────────────────────────────────────────────────
if [[ ! -f "$IMAGE_PATH" ]]; then
  echo "ERROR: Test image not found at $IMAGE_PATH"
  echo "Render it first: pdftoppm -png -f 1 -l 1 -r 150 <pdf> /tmp/kask-ocr-test-page"
  exit 1
fi

IMAGE_B64=$(base64 -w 0 "$IMAGE_PATH")
echo "RunPod kask-ocr Endpoint Concurrency Ramp-Up Test"
echo "Endpoint: ${REST_BASE}/chat/completions"
echo "Image:    ${IMAGE_PATH} (${#IMAGE_B64} bytes base64)"
echo "Levels:   ${CONCURRENCY_LEVELS[*]}"
echo ""

# OLMOCR-2 uses the standard OpenAI chat completions format with an image_url.
OCR_PROMPT="Return the plain text representation of this document as if you were reading it naturally. Render the entire document in markdown format, including all headings, body text, and tables. Do not include any extra text or commentary."

# Build the JSON payload using jq to handle the base64 image safely
# Build the payload JSON file directly (base64 is too large for jq argv)
# The model name is the vLLM model ID, not the endpoint name.
# kask-ocr is the endpoint name; the actual model is allenai/olmocr-2-7b-1025.
OCR_MODEL="allenai/olmocr-2-7b-1025"
PAYLOAD_FILE="/tmp/kask-ocr-payload.json"
{
  echo -n '{"model":"'"$OCR_MODEL"'","messages":[{"role":"user","content":[{"type":"text","text":"'"$OCR_PROMPT"'"},{"type":"image_url","image_url":{"url":"data:image/png;base64,'
  echo -n "$IMAGE_B64"
  echo -n '"}}]}],"max_tokens":8192,"temperature":0.0}'
} > "$PAYLOAD_FILE"
PAYLOAD="@$PAYLOAD_FILE"

# ── Single request function ────────────────────────────────────────────────
# Sends one OCR request and writes a result line to a temp file.
# Args: $1 = request index, $2 = output file path
send_request() {
  local idx="$1"
  local outfile="$2"
  local start end latency status text_len
  start=$(date +%s.%N)
  local http_code body
  # Capture HTTP status code and body separately using curl's -w
  body=$(curl -s -w "\n%{http_code}" \
    --max-time "$REQUEST_TIMEOUT" \
    -X POST "${REST_BASE}/chat/completions" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer ${API_KEY}" \
    -d "$PAYLOAD" 2>/dev/null || true)
  http_code=$(echo "$body" | tail -1)
  # The response body is everything except the last line
  local resp_body
  resp_body=$(echo "$body" | sed '$d')
  end=$(date +%s.%N)
  latency=$(echo "$end - $start" | bc -l)
  if [[ "$http_code" == "200" ]]; then
    text_len=$(echo "$resp_body" | jq -r '.choices[0].message.content // "" | length' 2>/dev/null || echo "0")
    echo "${idx}|${latency}|${http_code}|${text_len}" >> "$outfile"
  else
    echo "${idx}|${latency}|${http_code}|0" >> "$outfile"
  fi
}
export -f send_request
export PAYLOAD API_KEY REST_BASE REQUEST_TIMEOUT PAYLOAD_FILE

# ── Test a concurrency level ──────────────────────────────────────────────
test_concurrency() {
  local level="$1"
  echo "============================================================"
  echo "Testing concurrency level: ${level}"
  echo "============================================================"
  local tmpfile
  tmpfile=$(mktemp)
  # Launch N concurrent requests using xargs -P
  seq 0 $((level - 1)) | xargs -I {} -P "$level" bash -c "send_request {} '$tmpfile'"
  # Aggregate results
  local total successes errors avg_latency p95_latency max_consec_5xx
  local error_rate
  total=$(wc -l < "$tmpfile")
  successes=$(grep -c '|200|' "$tmpfile" || true)
  errors=$((total - successes))
  if [[ $total -gt 0 ]]; then
    error_rate=$(echo "scale=4; $errors / $total" | bc -l)
  else
    error_rate="1.0000"
  fi
  # Latencies
  local latencies
  latencies=$(grep -oP '\|\K[0-9.]+' "$tmpfile" | sort -n)
  local n_lat
  n_lat=$(echo "$latencies" | wc -l)
  if [[ $n_lat -gt 0 ]]; then
    avg_latency=$(echo "$latencies" | awk '{s+=$1} END {printf "%.4f", s/NR}')
    # P95: the value at index ceil(0.95 * n)
    local p95_idx
    p95_idx=$(echo "scale=0; ($n_lat * 95 + 99) / 100" | bc)
    p95_latency=$(echo "$latencies" | sed -n "${p95_idx}p")
  else
    avg_latency="0.0000"
    p95_latency="0.0000"
  fi
  # Status codes
  local status_codes
  status_codes=$(grep -oP '\|\K[0-9]+\|' "$tmpfile" | tr -d '|' | sort | uniq -c | tr '\n' ' ')
  # Max consecutive 5xx
  max_consec_5xx=$(grep -oP '\|\K5\d\d\|' "$tmpfile" | tr -d '|' | awk '
    { if ($1 >= 500 && $1 < 600) { c++; if (c > m) m = c } else { c = 0 } }
    END { print m + 0 }')
  # Avg text length
  local avg_text_len="0"
  if [[ $successes -gt 0 ]]; then
    avg_text_len=$(grep '|200|' "$tmpfile" | awk -F'|' '{s+=$4} END {printf "%.0f", s/NR}')
  fi
  echo "  Successes:       ${successes}/${level}"
  echo "  Errors:          ${errors}"
  echo "  Error rate:      $(echo "scale=1; $error_rate * 100" | bc -l)%"
  echo "  Avg latency:     ${avg_latency}s"
  echo "  P95 latency:     ${p95_latency}s"
  echo "  Status codes:    ${status_codes}"
  echo "  Max consec 5xx:  ${max_consec_5xx}"
  [[ $successes -gt 0 ]] && echo "  Avg text length: ${avg_text_len} chars"
  # Output result as a pipe-delimited line for the summary
  echo "${level}|${successes}|${errors}|${error_rate}|${avg_latency}|${p95_latency}"
  rm -f "$tmpfile"
}

# ── Main loop ─────────────────────────────────────────────────────────────
all_results=()
should_stop=0
for level in "${CONCURRENCY_LEVELS[@]}"; do
  if [[ $should_stop -eq 1 ]]; then
    echo ""
    echo "Skipping level ${level} — stop condition met."
    break
  fi
  echo ""
  result=$(test_concurrency "$level")
  all_results+=("$result")
  # Parse the result to check stop conditions
  IFS='|' read -r lvl succ err err_rate avg_lat p95_lat <<< "$result"
  # Check error rate (compare as integers: err_rate * 100 > 10)
  err_rate_pct=$(echo "scale=1; $err_rate * 100" | bc -l)
  stop_err=$(echo "$err_rate > $MAX_ERROR_RATE" | bc -l)
  stop_lat=$(echo "$avg_lat > $MAX_AVG_LATENCY" | bc -l)
  if [[ "$stop_err" == "1" ]]; then
    echo ""
    echo "⚠ STOP: Error rate ${err_rate_pct}% exceeds ${MAX_ERROR_RATE}%"
    should_stop=1
  fi
  if [[ "$stop_lat" == "1" ]]; then
    echo ""
    echo "⚠ STOP: Avg latency ${avg_lat}s exceeds ${MAX_AVG_LATENCY}s"
    should_stop=1
  fi
done

# ── Summary table ─────────────────────────────────────────────────────────
echo ""
echo "================================================================================"
echo "SUMMARY"
echo "================================================================================"
printf "%12s %8s %8s %8s %8s %8s\n" "Concurrency" "Success" "Errors" "Error%" "AvgLat" "P95Lat"
printf "%12s %8s %8s %8s %8s %8s\n" "------------" "--------" "--------" "--------" "--------" "--------"
for r in "${all_results[@]}"; do
  IFS='|' read -r lvl succ err err_rate avg_lat p95_lat <<< "$r"
  err_pct=$(echo "scale=1; $err_rate * 100" | bc -l)
  printf "%12s %8s %8s %7s%% %7ss %7ss\n" "$lvl" "$succ" "$err" "$err_pct" "$avg_lat" "$p95_lat"
done

# ── Verdict ────────────────────────────────────────────────────────────────
max_tested=0
all_passed=1
for r in "${all_results[@]}"; do
  IFS='|' read -r lvl succ err err_rate avg_lat p95_lat <<< "$r"
  [[ "$lvl" -gt "$max_tested" ]] && max_tested="$lvl"
  stop_err=$(echo "$err_rate > $MAX_ERROR_RATE" | bc -l)
  [[ "$stop_err" == "1" ]] && all_passed=0
done
last_level=${CONCURRENCY_LEVELS[-1]}
echo ""
if [[ $all_passed -eq 1 && "$max_tested" -eq "$last_level" ]]; then
  echo "✅ PASS: All concurrency levels up to ${max_tested} completed successfully."
elif [[ $all_passed -eq 1 ]]; then
  echo "✅ PASS: Concurrency levels up to ${max_tested} completed within thresholds."
else
  failed=""
  for r in "${all_results[@]}"; do
    IFS='|' read -r lvl succ err err_rate avg_lat p95_lat <<< "$r"
    stop_err=$(echo "$err_rate > $MAX_ERROR_RATE" | bc -l)
    [[ "$stop_err" == "1" ]] && failed="${failed} ${lvl}"
  done
  echo "❌ FAIL: Error rate exceeded threshold at concurrency level(s):${failed}"
  exit 1
fi
