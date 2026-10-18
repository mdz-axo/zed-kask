#!/usr/bin/env bash
# Read-only S13 loop-anatomy sweep over every live SKILL.md.
# For each skill, report whether the body carries:
#   init   — an initial-condition statement
#   target — a target-condition statement
#   check  — a named Check signal (gate id, "Check:" phase, or lisp_eval)
#   bound  — a bound (max rounds/retries/attempts, escalate-after-N, one retry)
#   act    — an Act re-entry or terminal action (re-enter, escalate, halt, stop)
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SKILLS="$ROOT/.agents/skills"

printf '%-28s %-5s %-7s %-6s %-6s %-5s %s\n' SKILL init target check bound act note
for d in "$SKILLS"/*/; do
    n="$(basename "$d")"; md="$d/SKILL.md"; [ -f "$md" ] || continue
    init=no; grep -qi 'initial condition' "$md" && init=yes
    tgt=no;  grep -qi 'target condition' "$md" && tgt=yes
    chk=no;  grep -qE '`[A-Z][A-Z0-9]*-[0-9A-Z]+`|^Check:|lisp_eval' "$md" && chk=yes
    bnd=no;  grep -qiE 'bound:|max[_ ](rounds|retries|attempts|steps)|at most (one|two|three|1|2|3)|one (re-|re-?trace|retry|correction)|within the bound|escalate after|after .* failures' "$md" && bnd=yes
    act=no;  grep -qiE 're-enter|reenter|escalate|halt|stop after|STOP\b|terminal' "$md" && act=yes
    note=""
    [ "$chk" = no ] && note="NO-CHECK-SIGNAL"
    [ "$bnd" = no ] && note="$note NO-BOUND"
    [ "$act" = no ] && note="$note NO-ACT"
    printf '%-28s %-5s %-7s %-6s %-6s %-5s %s\n' "$n" "$init" "$tgt" "$chk" "$bnd" "$act" "$note"
done
