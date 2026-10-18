#!/usr/bin/env bash
# Read-only: emit every pinned Lisp gate form with its owning skill and gate id.
# One record per form: <skill>\t<gate>\t<form>
# Gate id is taken from the nearest preceding backticked ALLCAPS-OR-DIGIT token
# on the same bullet or the bullet above.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SKILLS="$ROOT/.agents/skills"

for d in "$SKILLS"/*/; do
    name="$(basename "$d")"
    md="$d/SKILL.md"
    [ -f "$md" ] || continue
    awk -v skill="$name" '
        /`[A-Z][A-Z0-9]*-[0-9A-Z]+`/ {
            if (match($0, /`[A-Z][A-Z0-9]*-[0-9A-Z]+`/)) {
                gate = substr($0, RSTART+1, RLENGTH-2)
            }
        }
        /- form: `/ {
            line = $0
            sub(/^.*- form: `/, "", line)
            sub(/`[[:space:]]*$/, "", line)
            if (gate != "") print skill "\t" gate "\t" line
        }
    ' "$md"
done
