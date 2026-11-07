#!/usr/bin/env bash
# determinism-study-compare.sh — the identity observable for the engine-compute
# determinism study: per (label, class, cap) group, the N-way within-leg
# byte-identity of the COMBINED content+reasoning surface — the hardened
# p2-parity-compare discipline (`ac40450c3f`): this thinking model emits its
# tokens into reasoning_content first (finish_reason "length" with empty
# content is the normal shape at short caps), so BOTH fields participate; a
# rep whose combined surface is EMPTY is a typed failure — the empty-side
# comparison is refused, never adjudicated (never a vacuous identical); a
# group with no discovered bodies fails visibly (the zero-class guard — a
# green verdict over nothing is the broken-input class).
#
# Per-rep rows carry the rep-order data H4 reads: length, first-divergence
# depth vs rep1, differing bytes vs rep1, and the adjacent-pair depth
# (rep k vs rep k-1) so a monotone drift across a leg is visible.
#
# NOTE on compare_bytes: the p2-parity-compare.sh differing-count form
# (`cmp -l | wc -l || echo 0` under pipefail) never executed on a differing
# pair in P2 (the gate exited red before the band step) — it would yield a
# multiline value. This hardened form absorbs cmp's exit status before the
# pipe; it is the form that actually runs here.
#
# Usage: determinism-study-compare.sh LEG_DIR OUT_TSV
#   LEG_DIR — a determinism-study-leg.sh output dir (ONE leg: its first
#             *_summary.tsv is the record; one OUT_DIR per leg)
#   OUT_TSV — the per-rep identity rows; group verdicts go to stdout
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "usage: $0 LEG_DIR OUT_TSV" >&2
    exit 2
fi

leg_dir=$1
out_tsv=$2

summary=$(ls "$leg_dir"/*_summary.tsv 2>/dev/null | head -1)
if [ -z "$summary" ]; then
    echo "FATAL: no *_summary.tsv under $leg_dir — wrong dir? A verdict over no bodies is the no_classes_discovered class" >&2
    exit 5
fi

printf 'label\tclass\tcap\trep\tprompt_tokens\tcompletion_tokens\tfinish_reason\tcontent_chars\treasoning_chars\tcombined_chars\tvs_r1\tfirst_diff_r1\tdiff_bytes_r1\tvs_prev\tfirst_diff_prev\n' > "$out_tsv"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# The group list, derived from the summary TSV itself (field-aware — never a
# tab-collapsed display read): unique (label, class, cap) in first-seen order.
awk -F'\t' 'NR>1 { if (!seen[$1 SUBSEP $2 SUBSEP $3]++) print $1 "\t" $2 "\t" $3 }' "$summary" > "$tmp/groups.tsv"

group_count=$(wc -l < "$tmp/groups.tsv")
if [ "$group_count" -eq 0 ]; then
    echo "FATAL: zero groups discovered in $summary — no_classes_discovered, never a green verdict over an empty record" >&2
    exit 5
fi

extract_fields() { # BODYFILE OUT_PREFIX -> writes OUT_PREFIX.content.txt / .reasoning.txt
    jq -j '.choices[0].message.content // ""' "$1" > "${2}.content.txt" 2>/dev/null
    jq -j '.choices[0].message.reasoning_content // ""' "$1" > "${2}.reasoning.txt" 2>/dev/null
}

compare_bytes() { # FILE_A FILE_B -> "first_diff differing" (first_diff 0 = byte-identical)
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

while IFS=$'\t' read -r label class cap; do
    reps=$(awk -F'\t' -v l="$label" -v c="$class" -v k="$cap" 'NR>1 && $1==l && $2==c && $3==k {print $4}' "$summary" | sort -n)
    [ -n "$reps" ] || { echo "FATAL: group $label/$class/cap$cap lost its reps between discovery and read" >&2; exit 5; }

    combined_zero=0
    verdict=ALL_SAME
    prev_txt=""
    first_rep=""
    for rep in $reps; do
        body=$(printf '%s/%s_%s_cap%s_r%s_resp.json' "$leg_dir" "$label" "$class" "$cap" "$rep")
        if [ ! -f "$body" ]; then
            echo "FATAL: body $body named by the summary but absent — broken input" >&2
            exit 5
        fi
        extract_fields "$body" "$tmp/g_${rep}"
        cc=$(wc -c < "$tmp/g_${rep}.content.txt")
        rc=$(wc -c < "$tmp/g_${rep}.reasoning.txt")
        combined=$((cc + rc))
        [ "$combined" -eq 0 ] && combined_zero=1

        row=$(awk -F'\t' -v l="$label" -v c="$class" -v k="$cap" -v r="$rep" \
            'NR>1 && $1==l && $2==c && $3==k && $4==r {print $6 "\t" $7 "\t" $8; exit}' "$summary")

        if [ -z "$first_rep" ]; then
            first_rep=$rep
            vs_r1="same"; fd1="0"; db1="0"
        else
            if cmp -s "$tmp/g_${first_rep}.content.txt" "$tmp/g_${rep}.content.txt" && \
               cmp -s "$tmp/g_${first_rep}.reasoning.txt" "$tmp/g_${rep}.reasoning.txt"; then
                vs_r1="same"; fd1="0"; db1="0"
            else
                vs_r1="diff"
                envelope=$(compare_bytes "$tmp/g_${first_rep}.content.txt" "$tmp/g_${rep}.content.txt"; printf '\t'; compare_bytes "$tmp/g_${first_rep}.reasoning.txt" "$tmp/g_${rep}.reasoning.txt")
                fd1=$(printf '%s' "$envelope" | awk -F'\t' '{d1=$1; d2=$3; if (d1==0 && d2==0) print 0; else if (d1==0) print d2; else if (d2==0) print d1; else print (d1<d2?d1:d2)}')
                db1=$(printf '%s' "$envelope" | awk -F'\t' '{print $2+$4}')
                verdict=NOT_ALL_SAME
            fi
        fi

        if [ -z "$prev_txt" ]; then
            vs_prev="first"; fdp="-"
        else
            if cmp -s "$prev_txt.content.txt" "$tmp/g_${rep}.content.txt" && \
               cmp -s "$prev_txt.reasoning.txt" "$tmp/g_${rep}.reasoning.txt"; then
                vs_prev="same"; fdp="0"
            else
                vs_prev="diff"
                envelope=$(compare_bytes "$prev_txt.content.txt" "$tmp/g_${rep}.content.txt"; printf '\t'; compare_bytes "$prev_txt.reasoning.txt" "$tmp/g_${rep}.reasoning.txt")
                fdp=$(printf '%s' "$envelope" | awk -F'\t' '{d1=$1; d2=$3; if (d1==0 && d2==0) print 0; else if (d1==0) print d2; else if (d2==0) print d1; else print (d1<d2?d1:d2)}')
            fi
        fi

        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$label" "$class" "$cap" "$rep" "$row" "$cc" "$rc" "$combined" \
            "$vs_r1" "$fd1" "$db1" "$vs_prev" "$fdp" >> "$out_tsv"

        prev_txt="$tmp/g_${rep}"
    done

    if [ "$combined_zero" -eq 1 ]; then
        verdict=EMPTY_SIDE_REFUSED
    fi
    echo "VERDICT $label $class cap$cap reps=$(printf '%s\n' $reps | wc -l) $verdict"
done < "$tmp/groups.tsv"

echo "COMPARE_DONE -> $out_tsv"
