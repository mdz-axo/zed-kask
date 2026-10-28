#!/usr/bin/env bash
# Read-only: reconcile every ontology anchor a SKILL.md cites against the
# live registry, for the `onto_anchor` → derived form.
#
# Two defects this instrument had, both fixed here:
#   1. The pattern matched a bare `derived \`key\`` anywhere, so a skill
#      writing "the EQM-derived `overconfidence_bias`" was read as an
#      ontology anchor. The pattern now requires the `onto_anchor` context
#      immediately before `derived`.
#   2. `grep -o` exits 1 on no-match and `set -o pipefail` propagated that,
#      so a clean sweep exited 1 — a green result that read as failure.
#      Matching now runs through a helper that tolerates no-match, and the
#      exit code reports the real condition: 1 only when a citation dangles.
#
# Reports <skill>\t<key>\t<status>. Exit 0 = every cited key resolves.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SKILLS="$ROOT/.agents/skills"
DERIVED="$ROOT/kask/crates/hkask-bridge-ontology/src/derived.rs"

[ -f "$DERIVED" ] || { echo "MISSING derived.rs"; exit 1; }

missing=0
total=0
for d in "$SKILLS"/*/; do
    n="$(basename "$d")"; md="$d/SKILL.md"; [ -f "$md" ] || continue
    # The `onto_anchor` → derived `key` form only. A skill's own prose about
    # something "EQM-derived" is not an ontology citation.
    keys="$(grep -o -E 'onto_anchor`? *(→|->) *derived `[a-z0-9_]+`' "$md" 2>/dev/null \
            | sed 's/.*derived `//;s/`$//' | sort -u || true)"
    [ -n "$keys" ] || continue
    while IFS= read -r key; do
        [ -n "$key" ] || continue
        total=$((total + 1))
        if grep -q "\"$key\"" "$DERIVED"; then
            printf '%s\t%s\tOK\n' "$n" "$key"
        else
            printf '%s\t%s\tMISSING\n' "$n" "$key"
            missing=$((missing + 1))
        fi
    done <<< "$keys"
done

echo "checked $total onto_anchor→derived citations; $missing dangling."
[ "$missing" -eq 0 ]
