#!/usr/bin/env bash
# Read-only: reconcile every project-document path cited in a SKILL.md
# against the tree. A cited doc that does not exist is a broken reference
# model; the skill points a reader at evidence that is not there.
#
# `grep -o` exits 1 on no-match and `set -o pipefail` propagated that, so a
# clean sweep exited 1 — a green result that read as failure. The per-skill
# grep tolerates no-match here, and the exit code reports the real condition:
# 1 only when a cited document is absent.
#
# Reports <skill>\t<status>\t<path>. Exit 0 = every cited doc resolves.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SKILLS="$ROOT/.agents/skills"

missing=0
total=0
for d in "$SKILLS"/*/; do
    n="$(basename "$d")"; md="$d/SKILL.md"; [ -f "$md" ] || continue
    paths="$(grep -o -E 'kask/[a-zA-Z0-9_./-]+\.md' "$md" 2>/dev/null | sort -u || true)"
    [ -n "$paths" ] || continue
    while IFS= read -r p; do
        [ -n "$p" ] || continue
        total=$((total + 1))
        if [ -f "$ROOT/$p" ]; then
            printf '%s\tOK\t%s\n' "$n" "$p"
        else
            printf '%s\tMISSING\t%s\n' "$n" "$p"
            missing=$((missing + 1))
        fi
    done <<< "$paths"
done

echo "checked $total in-tree doc citations; $missing missing."
[ "$missing" -eq 0 ]
