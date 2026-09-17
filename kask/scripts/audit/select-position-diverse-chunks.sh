#!/usr/bin/env bash
set -euo pipefail

# expect: I can build a bounded pilot across sources without defaulting to each source's first chunk.
# [P3] Motivating: Generative Space — pilot evidence represents source bodies rather than positional front matter.
# [P1] Constraining: Human Agency — selected records retain their complete source and identity metadata.
# [P2] Constraining: Cognitive Sovereignty — malformed or unclassified rows fail instead of entering the pilot.
# pre: input is nonempty current-protocol TaggedChunk JSONL with unique entity_ref values
# post: output contains up to K interior-quantile records per source, byte-for-byte equivalent as JSON values to input records

if [[ $# -ne 3 ]]; then
    echo "usage: $0 <tagged-chunks-jsonl> <output-jsonl> <chunks-per-source>" >&2
    exit 64
fi

input=$1
output=$2
chunks_per_source=$3

if [[ ! -f "$input" ]]; then
    echo "tagged chunks JSONL does not exist: $input" >&2
    exit 66
fi
if [[ ! "$chunks_per_source" =~ ^[1-9][0-9]*$ ]]; then
    echo "chunks-per-source must be a positive integer" >&2
    exit 64
fi
if [[ -e "$output" ]]; then
    echo "refusing to overwrite output: $output" >&2
    exit 73
fi
if ! command -v jq >/dev/null 2>&1; then
    echo "required command not found: jq" >&2
    exit 69
fi

mkdir -p "$(dirname "$output")"
tmp_output=$(mktemp "${output}.tmp.XXXXXX")
trap 'rm -f "$tmp_output"' EXIT

jq -s -e --argjson requested "$chunks_per_source" '
  if length == 0 then
    error("tagged chunks JSONL is empty")
  elif any(.[];
    ((.entity_ref | type) != "string") or
    ((.entity_ref | test(":[0-9]+$")) | not) or
    ((.source | type) != "string") or
    ((.source | length) == 0) or
    ((.text | type) != "string") or
    ((.text | length) == 0) or
    (.classification.status != "classified") or
    (.classification.ontology_protocol != "published-term-resolution-v1")) then
    error("every row must be a nonblank current-protocol classified chunk with an ordinal entity_ref")
  elif ([.[].entity_ref] | length) != ([.[].entity_ref] | unique | length) then
    error("duplicate entity_ref in tagged chunks JSONL")
  else
    group_by(.source)
    | map(
        sort_by(.entity_ref | capture(":(?<ordinal>[0-9]+)$").ordinal | tonumber) as $rows
        | ($rows | length) as $count
        | if $requested >= $count then
            $rows
          else
            [range(0; $requested) as $index
             | $rows[(((($index + 1) * $count) / ($requested + 1)) | floor)]]
          end)
    | flatten
    | .[]
  end
' "$input" | jq -c '.' > "$tmp_output"

input_rows=$(wc -l < "$input")
selected_rows=$(wc -l < "$tmp_output")
source_count=$(jq -s '[.[].source] | unique | length' "$input")
selected_sources=$(jq -s '[.[].source] | unique | length' "$tmp_output")
chunk_zero_count=$(jq -s '[.[] | select(.entity_ref | endswith(":0"))] | length' "$tmp_output")

if (( selected_sources != source_count )); then
    echo "selection lost source coverage: expected $source_count, selected $selected_sources" >&2
    exit 65
fi

mv "$tmp_output" "$output"
trap - EXIT
jq -n \
    --arg input "$input" \
    --arg output "$output" \
    --argjson input_rows "$input_rows" \
    --argjson selected_rows "$selected_rows" \
    --argjson sources "$source_count" \
    --argjson chunks_per_source "$chunks_per_source" \
    --argjson chunk_zero_count "$chunk_zero_count" \
    '{input:$input, output:$output, input_rows:$input_rows, selected_rows:$selected_rows, sources:$sources, chunks_per_source:$chunks_per_source, chunk_zero_count:$chunk_zero_count, strategy:"interior_quantiles"}'
