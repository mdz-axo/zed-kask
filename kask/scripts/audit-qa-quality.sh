#!/usr/bin/env bash
# Read-only QA evidence audit: mechanical grounding-verify subset, NOT a
# semantic quality certificate. See audit-qa-quality.jq and the corpus skill.
# Usage: audit-qa-quality.sh <qa.jsonl> <source-chunks.jsonl>
# stdout: JSON report (including error rows); stderr: input/tool failures.
# Exit 0 = report complete in this narrow scope, 1 = high citation findings,
# 2 = missing checks/data, 64 = usage. No exit status authorizes a live run.
# The withdrawn overlap threshold argument is intentionally not accepted.
set -euo pipefail
export LC_ALL=C

if [[ $# != 2 ]]; then
    echo "usage: audit-qa-quality.sh <qa.jsonl> <source-chunks.jsonl>" >&2
    exit 64
fi
for input in "$@"; do
    if [[ ! -f "$input" || ! -r "$input" ]]; then
        echo "audit input is not a readable file: $input" >&2
        exit 2
    fi
done
command -v jq >/dev/null || { echo 'audit requires jq' >&2; exit 2; }
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf -- "$WORK"' EXIT

# Retain every physical JSONL row, including malformed/blank/error rows.
# Embedded newlines remain inside strings; each output line has one envelope.
parse_rows() {
    jq -Rnc '[inputs] | to_entries[] | . as $row |
        try {line:($row.key+1), value:($row.value|fromjson), parse_error:null}
        catch {line:($row.key+1), value:null, parse_error:.}' "$1"
}
parse_rows "$1" > "$WORK/qa.jsonl"
parse_rows "$2" > "$WORK/chunks.jsonl"

# Six-gram DOCUMENT frequency: dedup (phrase, row), never count repetition
# within one answer twice or let a gram cross row boundaries. This diagnostic
# does not decide whether a repeated phrase is source text or contamination.
jq -r 'select(.parse_error == null) | .value |
    select(type == "object") | select(.error? == null) |
    (if has("response") then .response else . end) |
    select(type == "object") | select(.error? == null) | .output |
    select(type == "string" and test("\\S")) | gsub("[\\r\\n]"; " ")' \
    "$WORK/qa.jsonl" > "$WORK/answers.txt"
awk '{gsub(/[^a-zA-Z0-9]+/," "); $0=tolower($0);
      for(i=1;i<=NF-5;i++){s=$i; for(j=1;j<6;j++) s=s" "$(i+j); print s"|"NR}}' \
    "$WORK/answers.txt" | sort -u | cut -d'|' -f1 | sort | uniq -c | sort -rn \
    > "$WORK/df6.txt"
n_answers=$(wc -l < "$WORK/answers.txt")
jq -Rn --argjson n "$n_answers" '
    [inputs | capture("^ *(?<count>[0-9]+) (?<phrase>.*)$") |
      {document_frequency:(.count|tonumber),phrase}] as $grams |
    ($n*0.05|ceil) as $threshold |
    {answer_documents:$n, threshold_fraction:0.05, threshold_documents:$threshold,
     repeated_sixgrams:([$grams[]|select(.document_frequency >= $threshold)]|length),
     top:([$grams[]|select(.document_frequency >= $threshold)][0:10]),
     limitation:"ASCII-normalized six-gram document frequency is a repetition signal, not semantic contamination proof; source-phrase attribution is unperformed."}' \
    "$WORK/df6.txt" > "$WORK/boilerplate.json"

jq -n --arg qa_path "$1" --arg chunks_path "$2" \
    --slurpfile qa "$WORK/qa.jsonl" --slurpfile chunks "$WORK/chunks.jsonl" \
    --slurpfile boilerplate "$WORK/boilerplate.json" \
    -f "$SCRIPT_DIR/audit-qa-quality.jq" > "$WORK/report.json"
# Validate the registry contract even on error reports; this is not a
# user-controllable provenance label or a substitute for semantic verification.
jq -e '
    [.rows[].verified_claims[]] | all(.[];
      (.provenance as $p | ["tool_verified","platform_derived","model_inference",
       "unavailable","tool_no_match","pending_check","rejected"] | index($p) != null)
      and (.why|length)>=40)' "$WORK/report.json" >/dev/null
cat "$WORK/report.json"
case $(jq -r .status "$WORK/report.json") in
    findings) exit 1 ;;
    incomplete) exit 2 ;;
    mechanical_checks_completed) exit 0 ;;
    *) echo 'unexpected audit status' >&2; exit 2 ;;
esac
