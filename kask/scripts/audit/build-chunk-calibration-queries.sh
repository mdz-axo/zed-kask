#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
    echo "usage: $0 <extracted-txt-dir> <output-jsonl> <max-queries>" >&2
    exit 64
fi

input_dir=$1
output=$2
max_queries=$3

if [[ ! -d "$input_dir" ]]; then
    echo "input directory does not exist: $input_dir" >&2
    exit 66
fi
if [[ ! "$max_queries" =~ ^[1-9][0-9]*$ ]]; then
    echo "max-queries must be a positive integer" >&2
    exit 64
fi
if [[ -e "$output" ]]; then
    echo "refusing to overwrite output: $output" >&2
    exit 73
fi
if ! command -v jq >/dev/null 2>&1; then
    echo "jq is required" >&2
    exit 69
fi

mkdir -p "$(dirname "$output")"
tmp_output=$(mktemp "${output}.tmp.XXXXXX")
tmp_candidates=$(mktemp)
tmp_line=$(mktemp)
tmp_quote=$(mktemp)
trap 'rm -f "$tmp_output" "$tmp_candidates" "$tmp_line" "$tmp_quote"' EXIT

written=0
skipped=0
while IFS= read -r -d '' source_path; do
    (( written < max_queries )) || break
    awk '
        BEGIN { IGNORECASE = 1 }
        index($0, "\f") == 0 &&
        index($0, "\t") == 0 &&
        NF >= 12 && NF <= 80 &&
        length($0) >= 80 && length($0) <= 800 &&
        $0 !~ /<table|<tr|<td|<th|!\[/ &&
        $0 !~ /^[[:space:]]*(contents|index|bibliography|references)[[:space:]]*$/ &&
        $0 !~ /all rights reserved|copyright/ { print NR }
    ' "$source_path" > "$tmp_candidates"

    candidate_count=$(wc -l < "$tmp_candidates" | tr -d ' ')
    construction=middle_eligible_source_line_edge_words
    exact_line=true
    if (( candidate_count == 0 )); then
        awk '
            BEGIN { IGNORECASE = 1 }
            index($0, "\f") == 0 &&
            index($0, "\t") == 0 &&
            NF >= 12 && length($0) >= 160 &&
            $0 !~ /<table|<tr|<td|<th|!\[/ &&
            $0 !~ /^[[:space:]]*(contents|index|bibliography|references)[[:space:]]*$/ { print NR }
        ' "$source_path" > "$tmp_candidates"
        candidate_count=$(wc -l < "$tmp_candidates" | tr -d ' ')
        construction=middle_long_source_line_window_edge_words
        exact_line=false
    fi
    if (( candidate_count == 0 )); then
        ((skipped += 1))
        continue
    fi

    selected=$((candidate_count / 2 + 1))
    line_number=$(sed -n "${selected}p" "$tmp_candidates")
    sed -n "${line_number}p" "$source_path" > "$tmp_line"
    if [[ "$exact_line" == true ]]; then
        cp "$tmp_line" "$tmp_quote"
    else
        awk '{
            width = 600
            start = int((length($0) - width) / 2) + 1
            if (start < 1) start = 1
            fragment = substr($0, start, width)
            sub(/^[^[:space:]]+[[:space:]]+/, "", fragment)
            sub(/[[:space:]][^[:space:]]+$/, "", fragment)
            print fragment
        }' "$tmp_line" > "$tmp_quote"
    fi
    quote=$(cat "$tmp_quote")
    if [[ "$exact_line" == true ]]; then
        grep -F -x -m 1 -- "$quote" "$source_path" >/dev/null
    else
        grep -F -m 1 -- "$quote" "$source_path" >/dev/null
    fi

    query=$(awk '
        {
            edge = 6
            printf "Which source passage connects "
            for (i = 1; i <= edge && i <= NF; i++) printf "%s%s", (i == 1 ? "" : " "), $i
            printf " ... "
            start = NF - edge + 1
            if (start < edge + 1) start = edge + 1
            for (i = start; i <= NF; i++) printf "%s%s", (i == start ? "" : " "), $i
            printf "?"
        }
    ' "$tmp_quote")
    source=$(basename "$source_path")
    query_id=$(printf '%s\0%s\0%s' "$source" "$line_number" "$quote" | sha256sum | cut -c1-24)

    jq -cn \
        --arg query_id "source-line-$query_id" \
        --arg query "$query" \
        --arg source "$source" \
        --arg source_path "$source_path" \
        --arg evidence_quote "$quote" \
        --arg construction "$construction" \
        --argjson source_line "$line_number" \
        '{query_id:$query_id,query:$query,source:$source,source_path:$source_path,source_line:$source_line,evidence_quote:$evidence_quote,provenance:"source_derived",construction:$construction}' \
        >> "$tmp_output"
    ((written += 1))
done < <(find "$input_dir" -maxdepth 1 -type f -name '*.txt' -print0 | sort -z)

if (( written == 0 )); then
    echo "no eligible source lines found" >&2
    exit 65
fi

mv "$tmp_output" "$output"
printf 'queries_written=%d sources_skipped=%d output=%s\n' "$written" "$skipped" "$output" >&2
