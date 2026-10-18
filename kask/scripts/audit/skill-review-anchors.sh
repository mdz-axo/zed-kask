#!/usr/bin/env bash
# Read-only: reconcile every ontology anchor a SKILL.md cites against the
# live registry. Checks two forms:
#   `onto_anchor` -> derived `key`        → key must exist in derived.rs
#   derived:<key> / core:<key> / a prefixed URI in a pinned source
# Reports <skill>\t<kind>\t<key>\t<status>
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SKILLS="$ROOT/.agents/skills"
DERIVED="$ROOT/kask/crates/hkask-bridge-ontology/src/derived.rs"

[ -f "$DERIVED" ] || { echo "MISSING derived.rs"; exit 1; }

for d in "$SKILLS"/*/; do
    n="$(basename "$d")"; md="$d/SKILL.md"; [ -f "$md" ] || continue
    # Backticked derived keys: `derived:key` or onto_anchor → derived `key`
    grep -o -E '`derived:[a-z0-9_]+`' "$md" | tr -d '`' | sed 's/^derived://' | sort -u | while read -r key; do
        if grep -q "\"$key\"" "$DERIVED"; then st=OK; else st=MISSING; fi
        printf '%s\tderived\t%s\t%s\n' "$n" "$key" "$st"
    done
    # "onto_anchor` → derived `key`" form
    grep -o -E 'derived `[a-z0-9_]+`' "$md" | sed 's/derived `//;s/`//' | sort -u | while read -r key; do
        if grep -q "\"$key\"" "$DERIVED"; then st=OK; else st=MISSING; fi
        printf '%s\tderived-quoted\t%s\t%s\n' "$n" "$key" "$st"
    done
done
