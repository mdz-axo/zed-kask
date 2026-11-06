#!/usr/bin/env bash
# p2-parity-compare.sh — the fresh-slot instrument gate + the streamed-vs-
# resident parity band arithmetic, over the legs' saved bodies.
#
#   The comparison observes the model's ACTUAL generated surface at these
#   caps: this thinking model emits its tokens into `reasoning_content`
#   first (finish_reason "length" with empty `content` is the normal shape at
#   128/256/1024-token caps), so BOTH fields participate: `content` AND
#   `reasoning_content`. A class whose combined generated surface is EMPTY on
#   either side is REFUSED (the known-positive discipline — an empty-vs-empty
#   byte comparison is vacuously identical, the exact class the media-widget
#   is_some() gate warns about).
#
#   Step 1 (the instrument gate, FIRST): within each leg, the temperature-0
#   reps of every class must be BYTE-IDENTICAL on the combined surface (the
#   D-G confound's fix — the fresh-slot protocol). Any within-leg divergence:
#   the instrument is still confounded, no cross-leg identity claim, verdict
#   instrument_confounded (the MTP record's honest-recording discipline).
#
#   Step 2 (the band): resident vs streaming, per class (the p0 suite's
#   non-stream bodies P1/P2/P3 + the longleg's 1024-token leg): both fields
#   byte-compared. Identical requires BOTH fields byte-equal; divergence
#   reports the per-field envelope (common-prefix depth, differing bytes,
#   lengths) — the length mismatch alone is the strongest divergence form.
#
#   The comparison is ENGINE-level (resident experts vs three-tier streamed
#   experts), never transport-level: only the non-stream bodies participate.
#
# Usage: p2-parity-compare.sh RESIDENT_DIR STREAMING_DIR OUT_TSV
set -euo pipefail

if [ "$#" -ne 3 ]; then
    echo "usage: $0 RESIDENT_DIR STREAMING_DIR OUT_TSV" >&2
    exit 2
fi

resident_dir=$1
streaming_dir=$2
out_tsv=$3

printf 'leg\tclass\tfield\tr1_r2\tr1_r3\tinstrument_gate\tvs_resident\tcommon_prefix_bytes\tdiffering_bytes\tlen_a\tlen_b\tctok_a\tctok_b\n' > "$out_tsv"

extract_fields() { # BODYFILE OUT_PREFIX -> writes OUT_PREFIX.content.txt / .reasoning.txt
    jq -j '.choices[0].message.content // ""' "$1" > "${2}.content.txt" 2>/dev/null
    jq -j '.choices[0].message.reasoning_content // ""' "$1" > "${2}.reasoning.txt" 2>/dev/null
}
decode_ctok() { jq -r '.usage.completion_tokens // empty' "$1" 2>/dev/null; }

compare_bytes() { # FILE_A FILE_B -> "common_prefix differing len_a len_b"
    local a=$1 b=$2 first_diff=0 differing=0 cmp_out
    if cmp -s "$a" "$b"; then
        printf '0\t0'
    else
        cmp_out=$(cmp "$a" "$b" 2>&1 || true)
        first_diff=$(printf '%s' "$cmp_out" | grep -o 'byte [0-9]*' | head -1 | grep -o '[0-9]*' || echo 1)
        first_diff=$((first_diff - 1))
        differing=$(cmp -l "$a" "$b" 2>/dev/null | wc -l || echo 0)
        if [ "$differing" -eq 0 ]; then
            differing=$(( $(wc -c < "$b") - $(wc -c < "$a") ))
            [ "$differing" -lt 0 ] && differing=$(( -differing ))
        fi
        printf '%s\t%s' "$first_diff" "$differing"
    fi
    printf '\t%s\t%s' "$(wc -c < "$a")" "$(wc -c < "$b")"
}

body_at() { # DIR CLASS N -> the resolved body path (the glob's first match), rc 1 on none.
    # The glob must EXPAND — a quoted pattern would name a literal '*' file
    # and silently miss every p0 body (observed: only the unglobbed long
    # class was discovered).
    local pattern f
    if [ "$2" = long ]; then
        pattern=$(printf '%s/long_r%d.json' "$1" "$3")
    else
        pattern=$(printf '%s/*_%s_nonstream_%d_resp.json' "$1" "$2" "$3")
    fi
    for f in $pattern; do
        [ -e "$f" ] && { printf '%s' "$f"; return 0; }
        return 1
    done
    return 1
}

instrument_check() { # DIR LEG -> green|red|empty (writes rows)
    local dir=$1 leg=$2 class gate=green combined_len
    local classes=""
    for c in P1 P2 P3; do
        body_at "$dir" "$c" 1 >/dev/null 2>&1 && classes="$classes $c"
    done
    [ -f "$dir/long_r1.json" ] && classes="$classes long"

    for class in $classes; do
        local t1
        t1=$(decode_ctok "$(body_at "$dir" "$class" 1)")
        for field in content reasoning; do
            extract_fields "$(body_at "$dir" "$class" 1)" "$dir/.g1"
            extract_fields "$(body_at "$dir" "$class" 2)" "$dir/.g2"
            extract_fields "$(body_at "$dir" "$class" 3)" "$dir/.g3"
            combined_len=$(($(wc -c < "$dir/.g1.content.txt") + $(wc -c < "$dir/.g1.reasoning.txt")))
            if [ "$combined_len" -eq 0 ]; then
                printf '%s\t%s\t%s\t%s\t%s\tEMPTY_REFUSED\t\t\t\t\t\t%s\t\n' \
                    "$leg" "$class" "$field" "-" "-" "$t1" >> "$out_tsv"
                gate=empty
                continue
            fi
            local h12 h13
            cmp -s "$dir/.g1.$field.txt" "$dir/.g2.$field.txt" && h12=same || { h12=DIFF; gate=red; }
            cmp -s "$dir/.g1.$field.txt" "$dir/.g3.$field.txt" && h13=same || { h13=DIFF; gate=red; }
            printf '%s\t%s\t%s\t%s\t%s\t%s\t\t\t\t\t\t%s\t\n' \
                "$leg" "$class" "$field" "$h12" "$h13" "$gate" "$t1" >> "$out_tsv"
        done
        rm -f "$dir/.g1."* "$dir/.g2."* "$dir/.g3."*
    done
    echo "$gate"
}

resident_gate=$(instrument_check "$resident_dir" resident)
streaming_gate=$(instrument_check "$streaming_dir" streaming)
echo "# instrument_gate: resident=$resident_gate streaming=$streaming_gate" >&2

if [ "$resident_gate" != green ] || [ "$streaming_gate" != green ]; then
    echo "VERDICT instrument_${resident_gate}_${streaming_gate} — no cross-leg identity claim (the D-G discipline)" >&2
    exit 4
fi

# Step 2: the cross-leg band — rep1 vs rep1 (the reps are byte-identical
# within each leg per the gate), both fields.
for class in P1 P2 P3 long; do
    ra=$(body_at "$resident_dir" "$class" 1) || continue
    sa=$(body_at "$streaming_dir" "$class" 1) || continue
    ctok_a=$(decode_ctok "$ra"); ctok_b=$(decode_ctok "$sa")
    extract_fields "$ra" "$resident_dir/.x1"
    extract_fields "$sa" "$streaming_dir/.x1"
    for field in content reasoning; do
        envelope=$(compare_bytes "$resident_dir/.x1.$field.txt" "$streaming_dir/.x1.$field.txt")
        case "$envelope" in
            0$'\t'0*) verdict=identical ;;
            *) verdict=divergent ;;
        esac
        printf '%s\t%s\t%s\t%s\t%s\tgreen\t%s\t%s\t%s\t%s\n' \
            "cross" "$class" "$field" "same" "same" "$verdict" \
            "$envelope" "$ctok_a" "$ctok_b" >> "$out_tsv"
    done
    rm -f "$resident_dir/.x1."* "$streaming_dir/.x1."*
done

echo "VERDICT band_measured — see $out_tsv (the lisp receipts at record time)" >&2
