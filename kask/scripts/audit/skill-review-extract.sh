#!/usr/bin/env bash
# Read-only per-skill extraction for the skill-maintenance semantic review.
# Emits one compact record per live skill so the auditor reads the same
# sections for every skill instead of skimming full bodies unevenly.
#
# Usage: bash kask/scripts/audit/skill-review-extract.sh <skill-name> [...]
#        bash kask/scripts/audit/skill-review-extract.sh --all
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SKILLS="$ROOT/.agents/skills"
TEMPLATES="$ROOT/kask/registry/templates"

section() {
    # Print the named "## <heading>" section body, capped.
    awk -v want="$2" '
        /^## / { h=$0; sub(/^## /,"",h); inside = (index(h, want) == 1); next }
        inside { print }
    ' "$1" | head -n "${3:-14}"
}

one() {
    local name="$1" md="$SKILLS/$1/SKILL.md"
    echo "================ $name ================"
    if [ ! -f "$md" ]; then echo "MISSING SKILL.md"; return; fi
    local lines; lines=$(wc -l < "$md")
    local tpl; tpl=$(ls "$TEMPLATES/$name"/*.j2 2>/dev/null | wc -l)
    echo "lines=$lines templates=$tpl"

    echo "--- description ---"
    awk '/^description:/{p=1} p{print} /^---$/{if(p&&NR>3)exit}' "$md" | head -4

    echo "--- reference models ---"
    section "$md" "Reference model" 8

    echo "--- initial/target condition ---"
    section "$md" "Initial and target condition" 8

    echo "--- Check/Act vocabulary ---"
    grep -n -i 'bound:\|max rounds\|max attempts\|escalate\|re-enter\|reenter\|convergence\|Check signal\|stopping rule\|zero-delta' "$md" | head -8

    echo "--- deterministic forms (lisp_eval gates) ---"
    grep -n -o 'gate `[A-Z][A-Z0-9-]*`\|`[A-Z][A-Z0-9-]*-[0-9]` form' "$md" | head -12

    echo "--- templates referenced ---"
    grep -n -o '[a-z0-9-]*/[a-z0-9-]*\.j2' "$md" | sort -u | head -20

    echo "--- composition (skill mentions) ---"
    grep -n -o 'use `[a-z-]*`\|`[a-z-]*` skill\|delegate to [a-z-]*' "$md" | head -10

    echo "--- Registry Templates table ---"
    awk '/^## Registry Templates/{p=1;next} p&&/^## /{exit} p' "$md" | grep '^|' | head -14
    echo
}

if [ "${1:-}" = "--all" ]; then
    for d in "$SKILLS"/*/; do one "$(basename "$d")"; done
else
    for name in "$@"; do one "$name"; done
fi
