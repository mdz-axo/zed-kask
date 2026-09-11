#!/usr/bin/env bash
# audit-qa-quality.sh — semantic quality audit for corpus-generated QA pairs.
#
# Structure checks (valid JSON, non-empty, no exact duplicates) are NOT quality.
# This script checks semantics, per the repo .rules:
#   1. grounding rate   — fraction of answer 4-grams present in the source chunks,
#                         plus the per-answer distribution
#   2. boilerplate      — 6-grams repeated across many DISTINCT answers
#   3. qa_type spread   — distribution evenness (when metadata is present)
#   4. source diversity — distinct sources covered (when metadata is present)
#
# Accepts both QA shapes:
#   generated: {"response":{"output":...},"qa_type":...,"source":...}
#   training:  {"instruction":...,"output":...}
#
# Record boundaries are preserved: embedded newlines are folded to spaces and
# normalization runs per line, so no n-gram ever spans two answers or two chunks.
#
# Usage: audit-qa-quality.sh <qa.jsonl> <source-chunks.jsonl> [min_grounding]
#   min_grounding defaults to 0.60 (the .rules target).
set -euo pipefail

# comm(1) requires byte-order sorting; locale collation silently breaks it.
export LC_ALL=C

QA="${1:?usage: audit-qa-quality.sh <qa.jsonl> <source-chunks.jsonl> [min_grounding]}"
CHUNKS="${2:?usage: audit-qa-quality.sh <qa.jsonl> <source-chunks.jsonl> [min_grounding]}"
MIN_GROUNDING="${3:-0.60}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Generated shape first, training shape second; fold newlines so each record
# stays on exactly one line.
ANSWER_EXPR='if (.response.output? != null) then .response.output else .output end | gsub("\n"; " ")'
SOURCE_EXPR='.text | gsub("\n"; " ")'

emit_answers() { jq -r "$ANSWER_EXPR" "$QA"; }
emit_source()  { jq -r "$SOURCE_EXPR" "$CHUNKS"; }

# Normalize WITHIN each line: lowercase, every non-alphanumeric run to a space.
# Never use `tr -cs` here — it folds newlines and merges all records into one.
norm() { awk '{gsub(/[^a-zA-Z0-9]+/," "); print tolower($0)}'; }

# N-grams of the normalized word stream, one per line.
grams() { awk -v N="$1" '{for(i=1;i<=NF-N+1;i++){s=$i; for(j=1;j<N;j++) s=s" "$(i+j); print s}}'; }

# N-grams tagged with their record index (NR), for per-record accounting.
grams_idx() { awk -v N="$1" '{for(i=1;i<=NF-N+1;i++){s=$i; for(j=1;j<N;j++) s=s" "$(i+j); print s"|"NR}}'; }

echo "== QA quality audit =="
echo "qa file:     $QA"
echo "chunks file: $CHUNKS"
echo

# ── 1. Grounding rate ─────────────────────────────────────────────────────
emit_source  | norm | grams 4 | sort -u > "$WORK/src4.txt"
emit_answers | norm | grams 4 | sort -u > "$WORK/ans4.txt"

ans_total=$(wc -l < "$WORK/ans4.txt")
ans_grounded=$(comm -12 "$WORK/src4.txt" "$WORK/ans4.txt" | wc -l)
rate=$(awk -v g="$ans_grounded" -v t="$ans_total" 'BEGIN{printf "%.4f", (t>0? g/t : 0)}')

echo "-- 1. grounding rate (answer 4-grams found in source) --"
echo "   answer 4-grams:   $ans_total"
echo "   grounded 4-grams: $ans_grounded"
echo "   rate:             $rate  (target >= $MIN_GROUNDING)"
awk -v r="$rate" -v m="$MIN_GROUNDING" 'BEGIN{exit !(r+0 >= m+0)}' \
  && echo "   verdict:          PASS" \
  || echo "   verdict:          FAIL"

# Per-answer distribution: how grounded is a typical answer, and how many
# answers carry at least some verbatim overlap.
emit_answers | norm | grams_idx 4 | sort -u > "$WORK/ans_idx.txt"
sort -t'|' -k1,1 "$WORK/ans_idx.txt" > "$WORK/ans_idx_s.txt"
join -t'|' -1 1 -2 1 "$WORK/src4.txt" "$WORK/ans_idx_s.txt" > "$WORK/hits.txt"
cut -d'|' -f2 "$WORK/ans_idx.txt" | sort | uniq -c | awk '{print $2" "$1}' > "$WORK/tot.txt"
cut -d'|' -f2 "$WORK/hits.txt"    | sort | uniq -c | awk '{print $2" "$1}' > "$WORK/grd.txt"
awk 'NR==FNR{t[$1]=$2; next}{g[$1]=$2}
     END{n=0; ge1=0; ge10=0; ge30=0; ge60=0; sum=0;
         for(i in t){x=(i in g)?g[i]:0; f=x/t[i]; sum+=f; n++;
                     if(x>=1)ge1++; if(f>=0.10)ge10++; if(f>=0.30)ge30++; if(f>=0.60)ge60++}
         printf "   answers: %d\n", n;
         printf "   mean per-answer grounded fraction: %.4f\n", (n>0? sum/n : 0);
         printf "   answers with >=1 grounded 4-gram: %d (%.1f%%)\n", ge1,  (n>0? 100*ge1/n : 0);
         printf "   answers with fraction >= 0.10:    %d (%.1f%%)\n", ge10, (n>0? 100*ge10/n : 0);
         printf "   answers with fraction >= 0.30:    %d (%.1f%%)\n", ge30, (n>0? 100*ge30/n : 0);
         printf "   answers with fraction >= 0.60:    %d (%.1f%%)\n", ge60, (n>0? 100*ge60/n : 0)}' \
    "$WORK/tot.txt" "$WORK/grd.txt"
echo

# ── 2. Boilerplate contamination ──────────────────────────────────────────
# Document frequency of each 6-gram: how many DISTINCT answers contain it.
n_answers=$(emit_answers | wc -l)
emit_answers | norm | grams_idx 6 | sort -u | cut -d'|' -f1 | sort | uniq -c | sort -rn > "$WORK/df6.txt"

threshold=$(awk -v n="$n_answers" 'BEGIN{printf "%d", (n*0.05)+0.999}')
echo "-- 2. boilerplate (6-grams in >= 5% of answers, i.e. >= $threshold of $n_answers) --"
hot=$(awk -v t="$threshold" '$1>=t' "$WORK/df6.txt" | wc -l)
echo "   repeated 6-grams above threshold: $hot"
if [ "$hot" -gt 0 ]; then
  echo "   top offenders (doc_freq  phrase):"
  awk -v t="$threshold" '$1>=t' "$WORK/df6.txt" | head -10 | sed 's/^/     /'
fi
echo

# ── 3. qa_type spread ─────────────────────────────────────────────────────
if jq -e 'select(.qa_type? != null)' "$QA" >/dev/null 2>&1; then
  echo "-- 3. qa_type distribution --"
  jq -r 'select(.qa_type? != null) | .qa_type' "$QA" | sort | uniq -c | sort -rn | sed 's/^/   /'
  echo
fi

# ── 4. source diversity ───────────────────────────────────────────────────
if jq -e 'select(.source? != null)' "$QA" >/dev/null 2>&1; then
  echo "-- 4. source diversity --"
  echo "   distinct sources: $(jq -r 'select(.source? != null) | .source' "$QA" | sort -u | wc -l)"
  echo
fi