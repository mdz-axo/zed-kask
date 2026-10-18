#!/usr/bin/env bash
# Read-only: one compact review row per live skill, for the semantic review.
# Columns (TAB-separated):
#   skill  refmodels(240)  target(280)  checkact  gates  tpls  composition
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SKILLS="$ROOT/.agents/skills"
TEMPLATES="$ROOT/kask/registry/templates"

for d in "$SKILLS"/*/; do
    name="$(basename "$d")"
    md="$d/SKILL.md"
    [ -f "$md" ] || { printf '%s\tMISSING\t\t\t\t\t\n' "$name"; continue; }

    ref=$(awk '/^## Reference model/{p=1;next} p&&/^## /{exit} p' "$md" \
          | tr '\n' ' ' | sed 's/  */ /g' | cut -c1-240)
    tgt=$(awk '/^\*\*Target condition:\*\*|^- \*\*Target condition/{p=1} p{print} p&&/^$/{exit}' "$md" \
          | tr '\n' ' ' | sed 's/  */ /g' | cut -c1-280)
    ca=$(grep -o -i 'max_rounds\|max_retries\|max attempts\|max rounds\|escalate\|re-enter\|convergence\|zero-delta\|stopping\|bound:' "$md" \
         | tr 'A-Z' 'a-z' | sort -u | paste -sd, -)
    gates=$(grep -o -E '`[A-Z][A-Z0-9]*-[0-9A-Z]+`' "$md" | tr -d '`' | sort -u | paste -sd, -)
    tpls=$(ls "$TEMPLATES/$name"/*.j2 2>/dev/null | wc -l)
    comp=$(grep -o -E 'use `[a-z][a-z0-9-]*`|`[a-z][a-z0-9-]*` skill|delegate[sd]? to `?[a-z][a-z0-9-]*`?' "$md" \
           | sed 's/use `//;s/`//g;s/ skill//;s/delegated* to //' | sort -u | paste -sd, -)
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$ref" "$tgt" "$ca" "$gates" "$tpls" "$comp"
done
