#!/usr/bin/env bash
# OCR missing PDFs via the RunPod kask-ocr endpoint directly.
#
# The corpus MCP server's OCR path requires the Zed inference IPC bridge,
# which isn't available when the server runs as a standalone context server.
# This script renders each PDF page to an image, sends it to the RunPod
# endpoint, and writes the extracted text as a .txt file alongside the
# source.
#
# Usage: ./scripts/ocr-missing-pdfs.sh
# Requires: RUNPOD_API_KEY env var, pdftoppm, curl, jq, base64

set -euo pipefail

ENDPOINT_ID="hsldzov6932wf5"
API_KEY="${RUNPOD_API_KEY:?RUNPOD_API_KEY is not set}"
REST_BASE="https://api.runpod.ai/v2/${ENDPOINT_ID}/openai/v1"
OCR_MODEL="allenai/olmocr-2-7b-1025"
SOURCE_DIR="corpus/source-library"
OUTPUT_DIR="corpus/extracted/researcher"
OCR_PROMPT="Return the plain text representation of this document as if you were reading it naturally. Render the entire document in markdown format, including all headings, body text, and tables. Do not include any extra text or commentary."
REQUEST_TIMEOUT=120
MAX_PAGES=50  # safety cap — skip PDFs with more than 50 pages

mkdir -p "$OUTPUT_DIR"

# Find missing PDFs (source files without a corresponding .txt output)
missing_pdfs=()
for f in "$SOURCE_DIR"/*.pdf "$SOURCE_DIR"/*.PDF; do
  [ -f "$f" ] || continue
  base=$(basename "$f")
  out="${OUTPUT_DIR}/${base}.txt"
  if [ ! -f "$out" ]; then
    missing_pdfs+=("$f")
  fi
done

echo "Found ${#missing_pdfs[@]} missing PDFs to OCR"
echo ""

ocr_page() {
  local image_path="$1"
  local payload_file="$2"
  local response http_code text
  response=$(curl -s -w $'\n%{http_code}' \
    --max-time "$REQUEST_TIMEOUT" \
    -X POST "${REST_BASE}/chat/completions" \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer ${API_KEY}" \
    -d @"${payload_file}" 2>/dev/null || true)
  http_code=$(echo "$response" | tail -1)
  local body
  body=$(echo "$response" | sed '$d')
  if [ "$http_code" = "200" ]; then
    text=$(echo "$body" | jq -r '.choices[0].message.content // ""' 2>/dev/null || echo "")
    echo "$text"
  else
    echo "OCR_ERROR: HTTP $http_code - $(echo "$body" | head -c 200)" >&2
    echo ""
  fi
}

for pdf_path in "${missing_pdfs[@]}"; do
  base=$(basename "$pdf_path")
  out_file="${OUTPUT_DIR}/${base}.txt"
  echo "Processing: $base"

  # Get page count
  page_count=$(pdfinfo "$pdf_path" 2>/dev/null | grep "^Pages:" | awk '{print $2}')
  if [ -z "$page_count" ]; then
    page_count=$(pdftoppm -l 1 "$pdf_path" /dev/null 2>&1 | grep -oP '\d+' | head -1 || echo "1")
  fi
  page_count=${page_count:-1}

  if [ "$page_count" -gt "$MAX_PAGES" ]; then
    echo "  ⚠ Skipping — $page_count pages exceeds cap of $MAX_PAGES"
    echo "[SKIPPLED: $page_count pages exceeds cap of $MAX_PAGES]" > "$out_file"
    continue
  fi
  echo "  Pages: $page_count"

  # Render and OCR each page
  tmp_prefix="/tmp/kask-ocr-$(echo "$base" | md5sum | cut -c1-8)"
  all_text=""
  for page in $(seq 1 "$page_count"); do
    echo -n "  Page $page... "
    # Render page to PNG at 150 DPI
    pdftoppm -png -f "$page" -l "$page" -r 150 "$pdf_path" "$tmp_prefix" 2>/dev/null
    # Find the rendered image (pdftoppm pads page numbers with zeros)
    img_file="${tmp_prefix}-$(printf '%02d' "$page").png"
    if [ ! -f "$img_file" ]; then
      img_file="${tmp_prefix}-${page}.png"
    fi
    if [ ! -f "$img_file" ]; then
      echo "RENDER FAILED"
      continue
    fi

    # Build payload
    img_b64=$(base64 -w 0 "$img_file")
    payload_file="/tmp/kask-ocr-payload-${page}.json"
    {
      printf '{"model":"%s","messages":[{"role":"user","content":[{"type":"text","text":"%s"},{"type":"image_url","image_url":{"url":"data:image/png;base64,' "$OCR_MODEL" "$OCR_PROMPT"
      printf '%s' "$img_b64"
      printf '"}}]}],"max_tokens":8192,"temperature":0.0}'
    } > "$payload_file"

    # OCR the page
    page_text=$(ocr_page "$img_file" "$payload_file")
    if [ -n "$page_text" ]; then
      echo "OK ($(echo "$page_text" | wc -c) chars)"
      all_text="${all_text}${page_text}"$'\n\n--- PAGE BREAK ---\n\n'
    else
      echo "EMPTY"
    fi

    # Clean up
    rm -f "$img_file" "$payload_file"
  done

  # Write the output
  echo "$all_text" > "$out_file"
  echo "  Written: $out_file ($(wc -c < "$out_file") bytes)"
  echo ""
done

echo "Done. OCR'd ${#missing_pdfs[@]} PDFs."
