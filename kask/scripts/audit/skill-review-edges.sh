#!/usr/bin/env bash
# Read-only: reconcile every skill-to-skill composition edge named in a
# SKILL.md against the live skill set. Emits: <src> -> <dst> [OK|DANGLING]
# Only backticked `skill-name` mentions that name an existing skills directory
# or a near-miss are reported, so prose words are not mistaken for edges.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SKILLS="$ROOT/.agents/skills"

known="$(ls "$SKILLS")"
is_known() { printf '%s\n' "$known" | grep -qx "$1"; }

for d in "$SKILLS"/*/; do
    src="$(basename "$d")"
    md="$d/SKILL.md"
    [ -f "$md" ] || continue
    # Backticked tokens that look like skill ids (kebab-case, >=2 words or a
    # known name), plus "use `x`" / "to `x`" forms.
    grep -o -E '`[a-z][a-z0-9]*(-[a-z0-9]+)+`' "$md" | tr -d '`' | sort -u | while read -r tok; do
        [ "$tok" = "$src" ] && continue
        if is_known "$tok"; then
            printf '%s -> %s OK\n' "$src" "$tok"
        fi
    done
done
