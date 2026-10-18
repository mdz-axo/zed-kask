#!/usr/bin/env bash
# Read-only: print the convergence / loop section of the named skills, so a
# NO-BOUND / NO-ACT flag from the anatomy sweep can be read-triaged rather
# than believed (the sweep pattern under-matches alternative phrasings).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SKILLS="$ROOT/.agents/skills"

for n in "$@"; do
    md="$SKILLS/$n/SKILL.md"
    [ -f "$md" ] || { echo "### $n MISSING"; continue; }
    echo "################ $n"
    awk '/^#{2,4} .*(Convergence|Loop|PDCA|Check|Act)/{p=1} p&&/^## /&&!/Convergence|Loop|PDCA|Check|Act/{exit} p' "$md" \
        | head -n 22
    echo
done
