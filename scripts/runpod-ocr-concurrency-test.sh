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
# Requires: RUNPOD_API_KEY env var, curl, jq, xargs, bc

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
OCR_MODEL="allenai/olmocr-2-7b-1025"

# ── Prepare the image ─────────────────────────────────────────────────────
if [[ ! -f "$IMAGE_PATH" ]]; then
  echo "ERROR: Test image not found at $IMAGE_PATH"
  echo "Render it first: pdftoppm -png -f 1 -l 1 -r 150 <pdf> /tmp/kask-ocr-test-page"
  exit 1
fi

IMAGE_B64=$(base64 -w 0 "$IMAGE_PATH")
OCR_PROMPT="Return the plain text representation of this document as if you were reading it naturally. Render the entire document in markdown format, including all headings, body text, and tables. Do not include any extra text or commentary."

# Build the payload JSON file (base64 is too large for jq argv)
PAYLOAD_FILE="/tmp/kask-ocr-payload.json"
{
  printf '{"model":"%s","messages":[{"role":"user","content":[{"type":"text","text":"%s"},{"type":"image_url","image_url":{"url":"data:image/png;base64,' "$OCR_MODEL" "$OCR_PROMPT"
  printf '%s' "$IMAGE_B64"
  printf '"}}]}],"max_tokens":8192,"temperature":0.0}'
} > "$PAYLOAD_FILE"

echo "RunPod kask-ocr Endpoint Concurrency Ramp-Up Test"
echo "Endpoint: ${REST_BASE}/chat/completions"
echo "Image:    ${IMAGE_PATH} (${#IMAGE_B64} bytes base64)"
echo "Model:    ${OCR_MODEL}"
echo "Levels:   ${CONCURRENCY_LEVELS[*]}"
echo ""

# ── Single request function ──────────────────────────────────────────────
# Writes a result line to the output file: idx|latency|status|text_len
send_request() {
  local idx="$1"
  local outfile="$2"
  local start end latency http_code resp_body text_len
  start=$(date +%s.%N)
  # Capture HTTP status code and body using curl's -w
  local raw
  raw=$(curl -s -w $'\n%{http_code}' \
    --max-time "$REQUEST_TIMEOUT" \
    -X POST "${REST_BASE}/chat/completions" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer ${API_KEY}" \
    -d @"${PAYLOAD_FILE}" 2>/dev/null || true)
  http_code=$(echo "$raw" | tail -1)
  resp_body=$(echo "$raw" | sed '$d')
  end=$(date +%s.%N)
  latency=$(echo "$end - $start" | bc -l 2>/dev/null || echo "0")
  if [[ "$http_code" == "200" ]]; then
    text_len=$(echo "$resp_body" | jq -r '.choices[0].message.content // "" | length' 2>/dev/null || echo "0")
    printf '%s|%s|%s|%s\n' "$idx" "$latency" "$http_code" "$text_len" >> "$outfile"
  else
    printf '%s|%s|%s|%s\n' "$idx" "$latency" "$http_code" "0" >> "$outfile"
  fi
}
export -f send_request
export API_KEY REST_BASE REQUEST_TIMEOUT PAYLOAD_FILE

# ── Test a concurrency level ──────────────────────────────────────────────
# Prints progress to stderr, writes result line to stdout
test_concurrency() {
  local level="$1"
  echo "============================================================" >&2
  echo "Testing concurrency level: ${level}" >&2
  echo "============================================================" >&2
  local tmpfile
  tmpfile=$(mktemp)
  seq 0 $((level - 1)) | xargs -I {} -P "$level" bash -c "send_request {} '$tmpfile'"

  # Aggregate results
  local total successes errors
  total=$(wc -l < "$tmpfile")
  successes=$(grep -c '|200|' "$tmpfile" 2>/dev/null || echo "0")
  errors=$((total - successes))

  # Error rate
  local error_rate
  if [[ $total -gt 0 ]]; then
    error_rate=$(echo "scale=4; $errors / $total" | bc -l)
  else
    error_rate="1.0000"
  fi

  # Latencies (field 2, between first and second |)
  local latencies n_lat avg_latency p95_latency
  latencies=$(awk -F'|' '{print $2}' "$tmpfile" | sort -n)
  n_lat=$(echo "$latencies" | grep -c . || echo "0")
  if [[ $n_lat -gt 0 ]]; then
    avg_latency=$(echo "$latencies" | awk '{s+=$1} END {printf "%.4f", s/NR}')
    local p95_idx
    p95_idx=$(echo "scale=0; ($n_lat * 95 + 99) / 100" | bc)
    [[ $p95_idx -lt 1 ]] && p95_idx=1
    p95_latency=$(echo "$latencies" | sed -n "${p95_idx}p")
  else
    avg_latency="0.0000"
    p95_latency="0.0000"
  fi

  # Status codes
  local status_codes
  status_codes=$(awk -F'|' '{print $3}' "$tmpfile" | sort | uniq -c | tr '\n' ' ')

  # Max consecutive 5xx
  local max_consec_5xx
  max_consec_5xx=$(awk -F'|' '{print $3}' "$tmpfile" | awk '
    { if ($1 >= 500 && $1 < 600) { c++; if (c > m) m = c } else { c = 0 } }
    END { print m + 0 }')

  # Avg text length
  local avg_text_len="0"
  if [[ $successes -gt 0 ]]; then
    avg_text_len=$(grep '|200|' "$tmpfile" | awk -F'|' '{s+=$4} END {printf "%.0f", s/NR}')
  fi

  # Print progress to stderr
  echo "  Successes:       ${successes}/${level}" >&2
  echo "  Errors:          ${errors}" >&2
  echo "  Error rate:      $(echo "scale=1; $error_rate * 100" | bc -l)%" >&2
  echo "  Avg latency:     ${avg_latency}s" >&2
  echo "  P95 latency:     ${p95_latency}s" >&2
  echo "  Status codes:    ${status_codes}" >&2
  echo "  Max consec 5xx:  ${max_consec_5xx}" >&2
  [[ $successes -gt 0 ]] && echo "  Avg text length: ${avg_text_len} chars" >&2

  # Output result line to stdout (pipe-delimited)
  echo "${level}|${successes}|${errors}|${error_rate}|${avg_latency}|${p95_latency}|${max_consec_5xx}"
  rm -f "$tmpfile"
}

# ── Main loop ─────────────────────────────────────────────────────────────
all_results=()
should_stop=0
for level in "${CONCURRENCY_LEVELS[@]}"; do
  if [[ $should_stop -eq 1 ]]; then
    echo ""
    echo "Skipping level ${level} — stop condition met." >&2
    break
  fi
  echo ""
  result=$(test_concurrency "$level" 2>&1)
  # The last line of result is the data line (stderr was mixed in, but
  # test_concurrency writes data to stdout and progress to stderr)
  # Actually with 2>&1 they're mixed. Let me re-do this properly.
  # Re-run without 2>&1 - stderr goes to terminal, stdout captured
  result=$(test_concurrency "$level")
  all_results+=("$result")

  # Parse the result to check stop conditions
  IFS='|' read -r lvl succ err err_rate avg_lat p95_lat consec_5xx <<< "$result"
  stop_err=$(echo "$err_rate > $MAX_ERROR_RATE" | bc -l)
  stop_lat=$(echo "$avg_lat > $MAX_AVG_LATENCY" | bc -l)
  if [[ "$stop_err" == "1" ]]; then
    echo ""
    echo "⚠ STOP: Error rate $(echo "scale=1; $err_rate * 100" | bc -l)% exceeds ${MAX_ERROR_RATE}%"
    should_stop=1
  fi
  if [[ "$stop_lat" == "1" ]]; then
    echo ""
    echo "⚠ STOP: Avg latency ${avg_lat}s exceeds ${MAX_AVG_LATENCY}s"
    should_stop=1
  fi
  if [[ "$consec_5xx" -gt "$MAX_CONSECUTIVE_5XX" ]]; then
    echo ""
    echo "⚠ STOP: ${consec_5xx} consecutive 5xx errors"
    should_stop=1
  fi
done

# ── Summary table ─────────────────────────────────────────────────────────
echo ""
echo "================================================================================"
echo "SUMMARY"
echo "================================================================================"
printf "%12s %8s %8s %8s %10s %10s\n" "Concurrency" "Success" "Errors" "Error%" "AvgLat" "P95Lat"
printf "%12s %8s %8s %8s %10s %10s\n" "------------" "--------" "--------" "--------" "----------" "----------"
for r in "${all_results[@]}"; do
  IFS='|' read -r lvl succ err err_rate avg_lat p95_lat consec_5xx <<< "$r"
  err_pct=$(echo "scale=1; $err_rate * 100" | bc -l)
  printf "%12s %8s %8s %7s%% %9ss %9ss\n" "$lvl" "$succ" "$err" "$err_pct" "$avg_lat" "$p95_lat"
done

# ── Verdict ────────────────────────────────────────────────────────────────
max_tested=0
all_passed=1
for r in "${all_results[@]}"; do
  IFS='|' read -r lvl succ err err_rate avg_lat p95_lat consec_5xx <<< "$r"
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
    IFS='|' read -r lvl succ err err_rate avg_lat p95_lat consec_5xx <<< "$r"
    stop_err=$(echo "$err_rate > $MAX_ERROR_RATE" | bc -l)
    [[ "$stop_err" == "1" ]] && failed="${failed} ${lvl}"
  done
  echo "❌ FAIL: Error rate exceeded threshold at concurrency level(s):${failed}"
  exit 1
fi
