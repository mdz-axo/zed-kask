#!/usr/bin/env bash
# p2-parity-compare.sh — the fresh-slot instrument gate + the streamed-vs-
# resident parity band arithmetic, over the legs' saved bodies.
#
#   Step 1 (the instrument gate, FIRST): within each leg, the temperature-0
#   reps of every class must be BYTE-IDENTICAL (the D-G confound's fix — the
#   fresh-slot protocol). If any rep diverges within a leg, the instrument is
#   still confounded: the cross-leg comparison is NOT run and the verdict is
#   instrument_confounded (the MTP record's honest-recording discipline).
#
#   Step 2 (the band): resident vs streaming, per class (the p0 suite's
#   non-stream bodies P1/P2/P3 + the longleg's 1024-token leg), decoded
#   completions byte-compared: byte-identical, or the measured divergence
#   envelope — common-prefix depth (cmp's first differing byte), differing
#   byte count (cmp -l), total length, and the length-mismatch finding (a
#   completion-length divergence is the strongest divergence form).
#
#   The comparison is ENGINE-level (resident experts vs three-tier streamed
#   experts), never transport-level: only the non-stream bodies participate;
#   the p0 suite's SSE rep is excluded (transport variance is a separate
#   question).
#
# Usage: p2-parity-compare.sh RESIDENT_DIR STREAMING_DIR OUT_TSV
#   RESIDENT_DIR   the p0-bench out dir of the resident leg (label-prefixed bodies)
#   STREAMING_DIR  the p0-bench out dir of the streaming leg + long_rN.json from
#                  p2-parity-longleg.sh (same dir per the leg discipline)
#   OUT_TSV        the machine-readable verdict table (the lisp receipts'
#                  input at record time)
set -euo pipefail

if [ "$#" -ne 3 ]; then
    echo "usage: $0 RESIDENT_DIR STREAMING_DIR OUT_TSV" >&2
    exit 2
fi

resident_dir=$1
streaming_dir=$2
out_tsv=$3

printf 'leg\tclass\trep1\trep2\trep3\tinstrument_gate\tvs_resident\tcommon_prefix_bytes\tdiffering_bytes\tlen_a\tlen_b\tctok_a\tctok_b\n' > "$out_tsv"

decode_content() { jq -r '.choices[0].message.content // empty' "$1" 2>/dev/null; }
decode_ctok() { jq -r '.usage.completion_tokens // empty' "$1" 2>/dev/null; }

# compare_bytes FILE_A FILE_B -> emits the divergence envelope fields
# (common_prefix_bytes differing_bytes len_a len_b); byte-identical emits 0 0.
compare_bytes() {
    local a=$1 b=$2
    local cmp_out first_diff=0 differing=0
    if cmp -s "$a" "$b"; then
        printf '0\t0'
    else
        cmp_out=$(cmp "$a" "$b" 2>&1 || true)
        # "a b differ: byte N, line M" -> common prefix = N-1 bytes
        first_diff=$(printf '%s' "$cmp_out" | grep -o 'byte [0-9]*' | head -1 | grep -o '[0-9]*' || echo 1)
        first_diff=$((first_diff - 1))
        differing=$(cmp -l "$a" "$b" 2>/dev/null | wc -l || echo 0)
        if [ "$differing" -eq 0 ]; then
            # one file is a strict prefix of the other: the length mismatch
            # IS the divergence (cmp -l lists nothing past the shorter file)
            differing=$(( $(wc -c < "$b") - $(wc -c < "$a") ))
            [ "$differing" -lt 0 ] && differing=$(( -differing ))
        fi
        printf '%s\t%s' "$first_diff" "$differing"
    fi
    printf '\t%s\t%s' "$(wc -c < "$a")" "$(wc -c < "$b")"
}

# instrument_leg LEG_DIR LEG_LABEL -> checks within-leg rep determinism for
# every class it finds (p0 classes P1/P2/P3 + the longleg's long); emits the
# gate row; sets GATE_STATUS for the caller.
instrument_check() {
    local dir=$1 leg=$2 class glob contents=() gate=green
    local classes=""
    # the p0 suite's classes, discovered from the saved bodies (label-prefixed)
    for c in P1 P2 P3; do
        if ls "$dir"/*_${c}_nonstream_1_resp.json >/dev/null 2>&1; then
            classes="$classes $c"
        fi
    done
    # the longleg's class (long_rN.json, no label prefix)
    if ls "$dir"/long_r1.json >/dev/null 2>&1; then
        classes="$classes long"
    fi
    for class in $classes; do
        if [ "$class" = long ]; then
            glob="$dir/long_r%d.json"
        else
            glob="$dir"/*_${class}_nonstream_%d_resp.json
        fi
        local c1 c2 c3 t1 t2 t3
        c1=$(decode_content "$(printf "$glob" 1)")
        c2=$(decode_content "$(printf "$glob" 2)")
        c3=$(decode_content "$(printf "$glob" 3)")
        printf '%s' "$c1" > "$dir/.cmp_c1.txt"
        printf '%s' "$c2" > "$dir/.cmp_c2.txt"
        printf '%s' "$c3" > "$dir/.cmp_c3.txt"
        local h12 h13
        if cmp -s "$dir/.cmp_c1.txt" "$dir/.cmp_c2.txt"; then h12=same; else h12=DIFF; gate=red; fi
        if cmp -s "$dir/.cmp_c1.txt" "$dir/.cmp_c3.txt"; then h13=same; else h13=DIFF; gate=red; fi
        t1=$(decode_ctok "$(printf "$glob" 1)")
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t\t\t\t\t\t%s\n' \
            "$leg" "$class" "$h12" "$h13" "-" "$gate" "-" "-" "-" "-" "-" "-" "$t1" >> "$out_tsv"
    done
    rm -f "$dir/.cmp_c1.txt" "$dir/.cmp_c2.txt" "$dir/.cmp_c3.txt"
    echo "$gate"
}

resident_gate=$(instrument_check "$resident_dir" resident)
streaming_gate=$(instrument_check "$streaming_dir" streaming)

echo "# instrument_gate: resident=$resident_gate streaming=$streaming_gate" >&2

if [ "$resident_gate" != green ] || [ "$streaming_gate" != green ]; then
    echo "VERDICT instrument_confounded" >&2
    echo "# the fresh-slot instrument did not achieve within-leg determinism — no cross-leg identity claim (the D-G discipline)" >&2
    exit 4
fi

# Step 2: the cross-leg band, per class — resident rep1 vs streaming rep1
# (the reps are byte-identical within each leg per the gate).
for class in P1 P2 P3 long; do
    if [ "$class" = long ]; then
        ra="$resident_dir/long_r1.json"; sa="$streaming_dir/long_r1.json"
    else
        ra=$(ls "$resident_dir"/*_${class}_nonstream_1_resp.json 2>/dev/null | head -1)
        sa=$(ls "$streaming_dir"/*_${class}_nonstream_1_resp.json 2>/dev/null | head -1)
    fi
    [ -n "${ra:-}" ] && [ -n "${sa:-}" ] || continue
    decode_content "$ra" > "$resident_dir/.vs_r.txt"
    decode_content "$sa" > "$streaming_dir/.vs_s.txt"
    envelope=$(compare_bytes "$resident_dir/.vs_r.txt" "$streaming_dir/.vs_s.txt")
    ctok_a=$(decode_ctok "$ra"); ctok_b=$(decode_ctok "$sa")
    verdict=identical
    case "$envelope" in
        0$'\t'0*) verdict=identical ;;
        *) verdict=divergent ;;
    esac
    printf '%s\t%s\t%s\t%s\t%s\tgreen\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "cross" "$class" "same" "same" "-" "green" "$verdict" \
        "$envelope" "$ctok_a" "$ctok_b" >> "$out_tsv"
    rm -f "$resident_dir/.vs_r.txt" "$streaming_dir/.vs_s.txt"
done

echo "VERDICT band_measured — see $out_tsv (identical vs divergent per class; the lisp receipts at record time)" >&2
