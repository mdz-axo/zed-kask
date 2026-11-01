#!/usr/bin/env bash
# R-A-r2 — frame-exact cut-plan generator (30 fps, integer frame space).
#
# Usage: gen-cut-plan.sh <cues.json> <out.json>
#
# Every boundary is chosen as an integer frame index f, emitted as f/30.
# Feasibility model (all in frames):
#   |f - 30*t_source| <= 2      (checker tolerance 0.0667s = 2.001 frames)
#   start < end                 (positive duration)
#   start[i+1] - end[i] >= 1    (checker's one-full-frame separation)
# The generator picks the minimum achievable worst-case deviation (0.01-frame
# grid search), then minimizes the summed deviation by dynamic programming,
# and fails loudly if no assignment exists inside the 2-frame windows.
set -euo pipefail

cues=${1:?usage: gen-cut-plan.sh <cues.json> <out.json>}
out=${2:?usage: gen-cut-plan.sh <cues.json> <out.json>}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

jq -r '.[] | "\(.id) \(.start_s) \(.end_s)"' "$cues" > "$work/cues.txt"
jq -r '.[].id' "$cues" > "$work/ids.txt"

awk -v OUT="$out" -v STATS="$work/stats.json" -v FPS=30 '
function fceil(x){ y=int(x); return (y<x)?y+1:y }
function ffloor(x){ y=int(x); return (y>x)?y-1:y }
function dabs(x){ return x<0?-x:x }

# Earliest-schedule feasibility check inside +-L frames.
function feasible(L,   i,s,e,prev,los,his,loe,hie){
  prev=-1e18
  for(i=1;i<=n;i++){
    los=fceil(S[i]-L-1e-9); his=ffloor(S[i]+L+1e-9)
    loe=fceil(E[i]-L-1e-9); hie=ffloor(E[i]+L+1e-9)
    s=los; if(prev+1>s)s=prev+1
    if(s>his)return 0
    e=loe; if(s+1>e)e=s+1
    if(e>hie)return 0
    prev=e
  }
  return 1
}

{ n++; id[n]=$1; st[n]=$2; en[n]=$3 }

END{
  for(i=1;i<=n;i++){ S[i]=st[i]*FPS; E[i]=en[i]*FPS }

  # 1. Minimum achievable worst-case deviation (0.01-frame grid).
  L=-1
  for(k=0;k<=200;k++){ if(feasible(k/100.0)){ L=k/100.0; break } }
  if(L<0){ print "FAIL: no assignment exists inside the 2-frame windows" > "/dev/stderr"; exit 2 }

  for(i=1;i<=n;i++){
    los[i]=fceil(S[i]-L-1e-9); his[i]=ffloor(S[i]+L+1e-9)
    loe[i]=fceil(E[i]-L-1e-9); hie[i]=ffloor(E[i]+L+1e-9)
  }

  # 2. DP: Hc[i,e] = min summed deviation for prefix 1..i ending on frame e.
  for(i=1;i<=n;i++){
    for(e=loe[i];e<=hie[i];e++){
      best=1e18; bs=-1; bp=-1
      for(s=los[i];s<=his[i];s++){
        if(s+1>e)continue
        if(i==1){
          c=dabs(s-S[i])+dabs(e-E[i])
          if(c<best-1e-12 || (c<=best+1e-12 && (bs<0||s<bs))){ best=c; bs=s; bp=-1 }
        } else {
          for(pe=loe[i-1];pe<=hie[i-1];pe++){
            if(!((i-1,pe) in Hc))continue
            if(s<pe+1)continue
            c=Hc[i-1,pe]+dabs(s-S[i])+dabs(e-E[i])
            if(c<best-1e-12 || (c<=best+1e-12 && (bs<0||s<bs||(s==bs&&pe<bp)))){
              best=c; bs=s; bp=pe
            }
          }
        }
      }
      if(bs>=0){ Hc[i,e]=best; Hs[i,e]=bs; Hp[i,e]=bp }
    }
  }

  # 3. Backtrack.
  best=1e18; be=-1
  for(e=loe[n];e<=hie[n];e++){
    if(!((n,e) in Hc))continue
    if(Hc[n,e]<best-1e-12 || (Hc[n,e]<=best+1e-12 && (be<0||e<be))){ best=Hc[n,e]; be=e }
  }
  if(be<0){ print "FAIL: DP found no solution" > "/dev/stderr"; exit 3 }
  e=be
  for(i=n;i>=1;i--){ ef[i]=e; sf[i]=Hs[i,e]; if(i>1)e=Hp[i,e] }

  # 4. Fail-loud self-check against the stated properties, before writing.
  for(i=1;i<=n;i++){
    if(dabs(sf[i]/FPS-st[i])>0.0667+1e-9){ printf "FAIL: id %s start out of tolerance\n",id[i] > "/dev/stderr"; exit 4 }
    if(dabs(ef[i]/FPS-en[i])>0.0667+1e-9){ printf "FAIL: id %s end out of tolerance\n",id[i] > "/dev/stderr"; exit 4 }
    if(sf[i]>=ef[i]){ printf "FAIL: id %s nonpositive duration\n",id[i] > "/dev/stderr"; exit 4 }
    if(i<n && sf[i+1]-ef[i]<1){ printf "FAIL: id %s gap below one frame\n",id[i+1] > "/dev/stderr"; exit 4 }
  }

  # 5. Emit.
  printf "[\n" > OUT
  for(i=1;i<=n;i++){
    printf "  {\"id\": %d, \"start_s\": %.12f, \"end_s\": %.12f}%s\n", id[i], sf[i]/FPS, ef[i]/FPS, (i<n?",":"") > OUT
  }
  printf "]\n" >> OUT

  # 6. Stats (for the run report).
  ov=0; naive=0; moved=0; maxdev=0; mingap=1e18; maxgap=-1
  for(i=1;i<=n;i++){
    if(i<n && st[i+1]<en[i]) ov++
    if(i<n){
      a=int(S[i+1]+0.5); b=int(E[i]+0.5)
      if(a-b<1) naive++
    }
    d1=dabs(sf[i]-S[i]); d2=dabs(ef[i]-E[i])
    if(d1>maxdev)maxdev=d1
    if(d2>maxdev)maxdev=d2
    if(d1>0.5+1e-12)moved++
    if(d2>0.5+1e-12)moved++
    if(i<n){
      g=sf[i+1]-ef[i]
      if(g<mingap)mingap=g
      if(g>maxgap)maxgap=g
    }
  }
  printf "{\"segments\": %d, \"max_deviation_frames\": %.4f, \"boundaries_moved\": %d, \"source_overlapping_pairs\": %d, \"pairs_colliding_after_nearest_snap\": %d, \"min_gap_frames\": %d, \"max_gap_frames\": %d}\n", n, maxdev, moved, ov, naive, mingap, maxgap > STATS
  close(STATS)
  printf "generator: %d segments | max deviation %.4f frames | moved %d boundaries | overlaps %d | naive collisions %d | gaps %d..%d frames\n", n, maxdev, moved, ov, naive, mingap, maxgap
}
' "$work/cues.txt"

cat "$work/stats.json"

# 7. Independent post-check of the written bytes (tight epsilon: 1e-6).
jq -r '.[] | "\(.id) \(.start_s) \(.end_s)"' "$out" > "$work/plan.txt"
awk -v FPS=30 '
NR==FNR{ cid[NR]=$0; c++; next }
{ pid[++k]=$1; ps[k]=$2; pe[k]=$3 }
END{
  bad=0
  if(k!=c){ print "POSTCHECK FAIL: count " k " != " c; bad++ }
  for(i=1;i<=k && i<=c;i++) if(pid[i]!=cid[i]){ print "POSTCHECK FAIL: id " pid[i] " != " cid[i]; bad++ }
  for(i=1;i<=k;i++){
    a=ps[i]*FPS; b=a-int(a); if(b>0.5)b=1-b; if(b>1e-6){ print "POSTCHECK FAIL: start misaligned id " pid[i] " x30=" a; bad++ }
    a=pe[i]*FPS; b=a-int(a); if(b>0.5)b=1-b; if(b>1e-6){ print "POSTCHECK FAIL: end misaligned id " pid[i] " x30=" a; bad++ }
    if(ps[i]>=pe[i]){ print "POSTCHECK FAIL: nonpositive duration id " pid[i]; bad++ }
    if(i>1){ g=ps[i]-pe[i-1]; if(g<1/FPS-1e-6){ print "POSTCHECK FAIL: gap id " pid[i] " = " g; bad++ } }
  }
  if(bad==0) print "POSTCHECK: aligned=true gaps>=1frame=true ids/order=true (eps 1e-6)"
  exit (bad>0)?1:0
}
' "$work/ids.txt" "$work/plan.txt"
