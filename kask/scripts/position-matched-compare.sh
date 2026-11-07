#!/usr/bin/env bash
# position-matched-compare.sh — the position-matched identity comparison
# (the protocol:
# kask/docs/research/private-inference-position-matched-identity-protocol-2026-11-06.md).
# Two legs ran the SAME frozen p0-bench sequence; their request positions
# match by construction. This script compares rep-N to rep-N per class on
# the COMBINED content+reasoning surface (the thinking-model discipline),
# with the empty-side refusal (a rep whose combined surface is empty on
# either side is a typed failure — never adjudicated, never a vacuous
# identical) and the zero-group guard (a class with no discovered bodies
# fails visibly).
#
# The comparison is ENGINE-level: p0-bench's stream bodies are excluded
# (transport-level). The within-leg reps are NOT expected to be identical
# to each other — the position trajectory is deterministic per sequence,
# so rep-2 vs rep-1 differing is normal; ONLY the cross-leg rep-N vs rep-N
# comparison adjudicates.
#
# Usage: position-matched-compare.sh DIR_A LABEL_A DIR_B LABEL_B OUT_TSV
set -euo pipefail

if [ "$#" -ne 5 ]; then
    echo "usage: $0 DIR_A LABEL_A DIR_B LABEL_B OUT_TSV" >&2
    exit 2
fi

dir_a=$1 label_a=$2 dir_b=$3 label_b=$4 out_tsv=$5

printf 'class\trep\tverdict\tfirst_diff\tdiff_bytes\tlen_a\tlen_b\tctok_a\tctok_b\n' > "$out_tsv"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

extract_fields() { # BODYFILE OUT_PREFIX
    jq -j '.choices[0].message.content // ""' "$1" > "${2}.content.txt" 2>/dev/null
    jq -j '.choices[0].message.reasoning_content // ""' "$1" > "${2}.reasoning.txt" 2>/dev/null
}

# The hardened compare_bytes form (the p2 script's differing-count under
# pipefail never executed on a real divergence; this one has).
compare_bytes() { # FILE_A FILE_B -> "first_diff differing" (0 0 = identical)
    local a=$1 b=$2 cmp_out first_diff differing
    if cmp -s "$a" "$b"; then
        printf '0\t0'
        return
    fi
    cmp_out=$(cmp "$a" "$b" 2>&1 || true)
    first_diff=$(printf '%s' "$cmp_out" | awk '{for (i=1;i<=NF;i++) if ($i=="byte") {v=$(i+1); gsub(/[^0-9]/,"",v); print v; exit}}')
    [ -n "$first_diff" ] || first_diff=1
    first_diff=$((first_diff - 1))
    differing=$({ cmp -l "$a" "$b" 2>/dev/null || true; } | wc -l)
    if [ "$differing" -eq 0 ]; then
        differing=$(( $(wc -c < "$b") - $(wc -c < "$a") ))
        [ "$differing" -lt 0 ] && differing=$(( -differing ))
    fi
    printf '%s\t%s' "$first_diff" "$differing"
}

overall=PASS
for class in P1 P2 P3; do
    class_verdict=IDENTICAL
    for rep in 1 2 3; do
        body_a="$dir_a/${label_a}_${class}_nonstream_${rep}_resp.json"
        body_b="$dir_b/${label_b}_${class}_nonstream_${rep}_resp.json"
        if [ ! -f "$body_a" ] || [ ! -f "$body_b" ]; then
            echo "FATAL: matched body missing — a: $body_a b: $body_b (wrong dir/label? zero-group guard)" >&2
            exit 5
        fi
        extract_fields "$body_a" "$tmp/a"
        extract_fields "$body_b" "$tmp/b"
        comb_a=$(( $(wc -c < "$tmp/a.content.txt") + $(wc -c < "$tmp/a.reasoning.txt") ))
        comb_b=$(( $(wc -c < "$tmp/b.content.txt") + $(wc -c < "$tmp/b.reasoning.txt") ))
        ctok_a=$(jq -r '.usage.completion_tokens // empty' "$body_a" 2>/dev/null || true)
        ctok_b=$(jq -r '.usage.completion_tokens // empty' "$body_b" 2>/dev/null || true)
        if [ "$comb_a" -eq 0 ] || [ "$comb_b" -eq 0 ]; then
            verdict=EMPTY_SIDE_REFUSED
            overall=REFUSED
        elif cmp -s "$tmp/a.content.txt" "$tmp/b.content.txt" && \
             cmp -s "$tmp/a.reasoning.txt" "$tmp/b.reasoning.txt"; then
            verdict=IDENTICAL
            printf '%s\t%s\t%s\t0\t0\t%s\t%s\t%s\t%s\n' \
                "$class" "$rep" "$verdict" "$comb_a" "$comb_b" "$ctok_a" "$ctok_b" >> "$out_tsv"
            continue
        else
            verdict=DIVERGENT
            class_verdict=DIVERGENT
            overall=FAIL
        fi
        if [ "$verdict" = EMPTY_SIDE_REFUSED ]; then
            fd="-"; db="-"
        else
            envelope=$(compare_bytes "$tmp/a.content.txt" "$tmp/b.content.txt"; printf '\t'; compare_bytes "$tmp/a.reasoning.txt" "$tmp/b.reasoning.txt")
            fd=$(printf '%s' "$envelope" | awk -F'\t' '{d1=$1; d2=$3; if (d1==0 && d2==0) print 0; else if (d1==0) print d2; else if (d2==0) print d1; else print (d1<d2?d1:d2)}')
            db=$(printf '%s' "$envelope" | awk -F'\t' '{print $2+$4}')
        fi
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$class" "$rep" "$verdict" "$fd" "$db" "$comb_a" "$comb_b" "$ctok_a" "$ctok_b" >> "$out_tsv"
    done
    echo "VERDICT $class: $class_verdict (position-matched, rep-N vs rep-N)"
done

echo "OVERALL: $overall (see $out_tsv)"
[ "$overall" = PASS ]
