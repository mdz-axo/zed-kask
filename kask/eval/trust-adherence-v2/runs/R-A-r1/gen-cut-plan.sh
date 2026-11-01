#!/usr/bin/env bash
# gen-cut-plan.sh — frame-exact highlight-reel cut plan (trust-adherence-v2 R-A).
#
# Reads:   kask/eval/trust-adherence-v2/fixtures/r-a-cues.json (or $1)
# Writes:  cut-plan.json in this script's directory (runs/R-A-r1/)
#
# All decisions are made in integer frame space (30 fps, frame = t * 30):
#   1. True frame position of every boundary: F = t * 30.
#   2. Nearest frame: round(F), half-up. (The source has no half-ties:
#      no cue time is an odd multiple of 0.05 s; 0.5000 s is frame-exact.)
#   3. Repair pass over consecutive pairs: where the snapped gap is
#      < 1 frame (the source contains outright overlaps and sub-frame
#      gaps), choose (end', start') within +/- 2 frames of the TRUE
#      source positions, with start' - end' >= 1, minimizing
#      (max |dev from source|, sum |dev|, end', start') lexicographically.
#      +/- 2 frames is exactly the checker's tolerance budget
#      (2/30 s = 0.0666.. s <= 0.0667 s).
#   4. Emit seconds = frame / 30 at full double precision (the checker's
#      2e-5 integrality epsilon absorbs the <= 1 ulp serialization dust).
#
# Self-verification (fail-loud, writes nothing on failure):
#   frame space, before writing: segment count, id set/order,
#     |dev| <= 2 frames, consecutive gap >= 1 frame, positive durations;
#   seconds space, on the WRITTEN file (the serialization seam), at
#     eps 1e-9 — four orders tighter than the checker's 2e-5.

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
cues="${1:-$here/../../fixtures/r-a-cues.json}"
out="$here/cut-plan.json"

[ -f "$cues" ] || { echo "gen-cut-plan.sh: cues file not found: $cues" >&2; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "gen-cut-plan.sh: jq required" >&2; exit 1; }

# --- pass 1: frame-space plan + invariants, then emit -----------------------
jq '
def ab: if . < 0 then (0 - .) else . end;
def nfr: (. + 0.5) | floor;
def wlo: (. - 2) | ceil;
def whi: (. + 2) | floor;

. as $src
| [ .[] | {id: .id, s: (.start_s * 30), e: (.end_s * 30)} ]
| map(. + {sn: (.s | nfr), en: (.e | nfr)})
| reduce range(0; length - 1) as $i (.;
    if .[$i + 1].sn - .[$i].en >= 1 then .
    else
      . as $acc
      | ($acc[$i].e) as $E
      | ($acc[$i + 1].s) as $S
      | ($E | wlo) as $elo | ($E | whi) as $ehi
      | ($S | wlo) as $slo | ($S | whi) as $shi
      | [ range($elo; $ehi + 1) as $e2
        | range($slo; $shi + 1) as $s2
        | select($s2 - $e2 >= 1)
        | (($e2 - $E) | ab) as $de
        | (($s2 - $S) | ab) as $ds
        | {e2: $e2, s2: $s2, mx: ([$de, $ds] | max), sm: ($de + $ds)}
        ]
      | if length == 0
        then error("R-A: pair at index \($i) (source ids \($acc[$i].id)/\($acc[$i + 1].id)) has no repair within the +/-2-frame tolerance — source infeasible")
        else . end
      | sort_by([.mx, .sm, .e2, .s2])
      | .[0] as $pick
      | $acc
      | .[$i].en = $pick.e2
      | .[$i + 1].sn = $pick.s2
    end)
| if length != ($src | length)
  then error("R-A: segment count changed") else . end
| if [.[].id] != [$src[].id]
  then error("R-A: id set/order diverged from source") else . end
| if ([ .[] | (((.en - .e) | ab) <= 2.0000001) and (((.sn - .s) | ab) <= 2.0000001) ] | all | not)
  then error("R-A: a boundary moved beyond 2 frames") else . end
| . as $a
| if ([ range(0; length - 1) as $i | ($a[$i + 1].sn - $a[$i].en) >= 1 ] | all | not)
  then error("R-A: consecutive gap below 1 frame after repair") else . end
| if ([ .[] | .en > .sn ] | all | not)
  then error("R-A: non-positive duration") else . end
| map({id: .id, start_s: (.sn / 30), end_s: (.en / 30)})
' "$cues" > "$out" || { rm -f "$out"; exit 1; }

# --- pass 2: re-verify the WRITTEN file in seconds space ---------------------
jq -n --slurpfile plan "$out" --slurpfile src "$cues" '
def ab: if . < 0 then (0 - .) else . end;
($plan[0]) as $p | ($src[0]) as $c | ($p | length) as $n
| if $n != ($c | length) then error("verify: count mismatch") else . end
| if [$p[].id] != [$c[].id] then error("verify: id set/order mismatch") else . end
| ([ $p[] | .start_s, .end_s
    | ((. * 30) - (((. * 30) + 0.5) | floor)) as $d
    | ($d | ab) <= 0.000000001 ] | all) as $aligned
| ([ range(0; $n - 1) as $i
    | ($p[$i + 1].start_s - $p[$i].end_s) >= ((1 / 30) - 0.000000001) ] | all) as $gaps
| ([ range(0; $n) as $i
    | (($p[$i].start_s - $c[$i].start_s) | ab) <= ((2 / 30) + 0.000000001)
      and (($p[$i].end_s - $c[$i].end_s) | ab) <= ((2 / 30) + 0.000000001) ] | all) as $tol
| if ($aligned and $gaps and $tol)
  then "written-file check: aligned=\($aligned) gaps>=1frame=\($gaps) within-2-frames=\($tol)"
  else error("verify: seconds-space invariant failed on the written file") end
'

# --- pass 3: stats for the report -------------------------------------------
jq -n --slurpfile plan "$out" --slurpfile src "$cues" '
def ab: if . < 0 then (0 - .) else . end;
def r3: (. * 1000 + 0.5 | floor) / 1000;
($plan[0]) as $p | ($src[0]) as $c | ($p | length) as $n
| [ range(0; $n) as $i
    | {sf: (($p[$i].start_s * 30 + 0.5) | floor),
       ef: (($p[$i].end_s   * 30 + 0.5) | floor),
       sn0: (($c[$i].start_s * 30 + 0.5) | floor),
       en0: (($c[$i].end_s   * 30 + 0.5) | floor),
       ss: ($c[$i].start_s * 30),
       es: ($c[$i].end_s * 30)}
    ] as $fr
| [ range(0; $n - 1) as $i | ($fr[$i + 1].sf - $fr[$i].ef) ] as $gaps
| [ range(0; $n - 1) as $i | ($fr[$i + 1].sn0 - $fr[$i].en0) < 1 ] as $snapviol
| [ range(0; $n - 1) as $i | ($c[$i + 1].start_s - $c[$i].end_s) < 0 ] as $overlap
| {segments: $n,
   source_overlapping_pairs: ($overlap | map(select(.)) | length),
   pairs_needing_repair_after_nearest_snap: ($snapviol | map(select(.)) | length),
   moved_boundaries: ([ $fr[] | (.sf != .sn0), (.ef != .en0) ] | map(select(.)) | length),
   max_boundary_deviation_frames: ([ $fr[] | ((.sf - .ss) | ab), ((.ef - .es) | ab) ] | max | r3),
   min_consecutive_gap_frames: ($gaps | min | r3),
   max_consecutive_gap_frames: ($gaps | max | r3)}
'
echo "wrote $out"