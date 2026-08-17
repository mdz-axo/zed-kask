#!/usr/bin/env bash
# OCR scanned PDFs via the RunPod kask-ocr endpoint (direct API, bypassing corpus server).
# OCRs the first N pages of each scanned PDF (bounded to avoid hours of processing).
set -euo pipefail

ENDPOINT_ID="hsldzov6932wf5"
API_KEY="${RUNPOD_API_KEY:?RUNPOD_API_KEY not set}"
REST_BASE="https://api.runpod.ai/v2/${ENDPOINT_ID}/openai/v1"
OCR_MODEL="allenai/olmocr-2-7b-1025"
OCR_PROMPT="Return the plain text representation of this document as if you were reading it naturally. Render the entire document in markdown format."
MAX_PAGES=20
SOURCE_DIR="corpus/source-library"
OUTPUT_DIR="corpus/extracted/researcher"

# Find scanned PDFs still missing extraction
scanned=()
for f in "$SOURCE_DIR"/*.pdf "$SOURCE_DIR"/*.PDF; do
  [ -f "$f" ] || continue
  base=$(basename "$f")
  out="${OUTPUT_DIR}/${base}.txt"
  if [ ! -f "$out" ]; then
    scanned+=("$f")
  fi
done

echo "Found ${#scanned[@]} scanned PDFs to OCR"
echo ""

for src in "${scanned[@]}"; do
  base=$(basename "$src")
  out="${OUTPUT_DIR}/${base}.txt"
  total_pages=$(pdfinfo "$src" 2>/dev/null | grep "^Pages:" | awk '{print $2}')
  total_pages=${total_pages:-1}
  pages_to_ocr=$total_pages
  if [ "$total_pages" -gt "$MAX_PAGES" ]; then
    pages_to_ocr=$MAX_PAGES
  fi
  echo "Processing: $base ($total_pages pages, OCRing first $pages_to_ocr)"

  all_text=""
  for page in $(seq 1 "$pages_to_ocr"); do
    echo -n "  Page $page... "
    tmp_prefix="/tmp/kask-ocr-$(echo "$base" | md5sum | cut -c1-8)"
    pdftoppm -png -f "$page" -l "$page" -r 150 "$src" "$tmp_prefix" 2>/dev/null || true
    img_file="${tmp_prefix}-$(printf '%02d' "$page").png"
    if [ ! -f "$img_file" ]; then
      img_file="${tmp_prefix}-${page}.png"
    fi
    if [ ! -f "$img_file" ]; then
      echo "RENDER FAILED"
      continue
    fi
    img_b64=$(base64 -w 0 "$img_file")
    payload_file="/tmp/kask-ocr-payload-${page}.json"
    {
      printf '{"model":"%s","messages":[{"role":"user","content":[{"type":"text","text":"%s"},{"type":"image_url","image_url":{"url":"data:image/png;base64,' "$OCR_MODEL" "$OCR_PROMPT"
      printf '%s' "$img_b64"
      printf '"}}]}],"max_tokens":8192,"temperature":0.0}'
    } > "$payload_file"
    response=$(curl -s -w '\n%{http_code}' --max-time 120 \
      -X POST "${REST_BASE}/chat/completions" \
      -H "Content-Type: application/json" \
      -H "Authorization: Bearer ${API_KEY}" \
      -d @"${payload_file}" 2>/dev/null || true)
    http_code=$(echo "$response" | tail -1)
    body=$(echo "$response" | sed '$d')
    if [ "$http_code" = "200" ]; then
      text=$(echo "$body" | jq -r '.choices[0].message.content // ""' 2>/dev/null || echo "")
      chars=$(echo "$text" | wc -c)
      echo "OK ($chars chars)"
      all_text="${all_text}${text}
--- PAGE BREAK ---

"
    else
      echo "FAIL (HTTP $http_code)"
    fi
    rm -f "$img_file" "$payload_file"
  done

  echo "$all_text" > "$out"
  echo "  Written: $out ($(wc -c < "$out") bytes)"
  echo ""
done

# Final check
missing=0
for f in "$SOURCE_DIR"/*; do
  base=$(basename "$f")
  out="${OUTPUT_DIR}/${base}.txt"
  if [ ! -f "$out" ]; then
    echo "STILL MISSING: $base"
    missing=$((missing + 1))
  fi
done
echo "Total extracted: $((138 - missing)) / 138"