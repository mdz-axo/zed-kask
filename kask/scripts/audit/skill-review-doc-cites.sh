#!/usr/bin/env bash
# Read-only: reconcile every project-document path cited in a SKILL.md
# against the tree. A cited doc that does not exist is a broken reference
# model; the skill points a reader at evidence that is not there.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SKILLS="$ROOT/.agents/skills"

for d in "$SKILLS"/*/; do
    n="$(basename "$d")"; md="$d/SKILL.md"; [ -f "$md" ] || continue
    grep -o -E 'kask/[a-zA-Z0-9_./-]+\.md' "$md" | sort -u | while read -r p; do
        if [ -f "$ROOT/$p" ]; then st=OK; else st=MISSING; fi
        printf '%s\t%s\t%s\n' "$n" "$st" "$p"
    done
done
