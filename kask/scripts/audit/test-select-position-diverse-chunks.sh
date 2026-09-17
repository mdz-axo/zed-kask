#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
SELECTOR="$SCRIPT_DIR/select-position-diverse-chunks.sh"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

classified='{"status":"classified","ontology_protocol":"published-term-resolution-v1"}'
input="$WORK/tagged.jsonl"
for ordinal in 0 1 2 3 4; do
    jq -nc --arg ref "corpus:test:a:$ordinal" --argjson classification "$classified" \
        '{entity_ref:$ref,source:"a.txt",text:("A passage " + $ref),classification:$classification}' >> "$input"
done
for ordinal in 0 1; do
    jq -nc --arg ref "corpus:test:b:$ordinal" --argjson classification "$classified" \
        '{entity_ref:$ref,source:"b.txt",text:("B passage " + $ref),classification:$classification}' >> "$input"
done
jq -nc --argjson classification "$classified" \
    '{entity_ref:"corpus:test:c:7",source:"c.txt",text:"Only C passage",classification:$classification}' >> "$input"

one="$WORK/one.jsonl"
summary=$($SELECTOR "$input" "$one" 1)
jq -e '
  .selected_rows == 3 and .sources == 3 and
  .chunks_per_source == 1 and .chunk_zero_count == 0 and
  .strategy == "interior_quantiles"
' <<<"$summary" >/dev/null
jq -s -e '
  (map(.entity_ref) == ["corpus:test:a:2", "corpus:test:b:1", "corpus:test:c:7"])
' "$one" >/dev/null
jq -s --slurpfile selected "$one" -e '
  . as $input
  | [$selected[] as $row | any($input[]; . == $row)]
  | all(.[]; .)
' "$input" >/dev/null

three="$WORK/three.jsonl"
summary=$($SELECTOR "$input" "$three" 3)
jq -e '.selected_rows == 6 and .sources == 3 and .chunks_per_source == 3' <<<"$summary" >/dev/null
jq -s -e '
  (map(select(.source == "a.txt").entity_ref) == ["corpus:test:a:1", "corpus:test:a:2", "corpus:test:a:3"]) and
  (map(select(.source == "b.txt").entity_ref) == ["corpus:test:b:0", "corpus:test:b:1"]) and
  (map(select(.source == "c.txt").entity_ref) == ["corpus:test:c:7"])
' "$three" >/dev/null

invalid="$WORK/invalid.jsonl"
jq -nc '{entity_ref:"corpus:test:bad:0",source:"bad.txt",text:"Bad passage",classification:{status:"unverified"}}' > "$invalid"
if $SELECTOR "$invalid" "$WORK/invalid-output.jsonl" 1 >/dev/null 2>&1; then
    echo "selector accepted an unverified row" >&2
    exit 1
fi
if [[ -e "$WORK/invalid-output.jsonl" ]]; then
    echo "selector published output after validation failure" >&2
    exit 1
fi

printf '%s\n' "position-diverse selector tests passed"
