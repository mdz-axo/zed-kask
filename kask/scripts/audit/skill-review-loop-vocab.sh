#!/usr/bin/env bash
# Read-only: for the named skills, print every line carrying loop-bound or
# loop-act vocabulary, so a NO-BOUND / NO-ACT flag is read-triaged against
# the actual prose rather than the sweep's narrow pattern.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SKILLS="$ROOT/.agents/skills"

for n in "$@"; do
    md="$SKILLS/$n/SKILL.md"
    [ -f "$md" ] || { echo "### $n MISSING"; continue; }
    echo "################ $n"
    grep -n -iE 'bound|max[_ ]|at most|once|one (re-|re-?trace|retry|correction|pass)|re-?enter|escalat|halt|stop|terminal|converge|round|iterat|cycle' "$md" \
        | grep -viE '^[0-9]+:description|^[0-9]+:# ' | head -n 10
    echo
done
