#!/usr/bin/env bash
# Docs-frontmatter gate (DOCUMENTATION_STANDARDS.md §2): every Markdown file
# under kask/docs opens with a YAML frontmatter block delimited by '---' and
# carrying all seven mandatory fields. The 2026-10-05 deep-alignment run
# found two concurrent-stream additions landing without the header (the
# compaction spec, the two-symptom findings) — this gate is the enforcement:
# the addition class fails CI instead of the next alignment run.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
DOCS_ROOT=${DOCS_ROOT:-"$root/kask/docs"}

fail() {
    echo "FAIL: $1" >&2
    [ "${2:-}" ] && printf '      %s\n' "$2" >&2
    exit 1
}

[ -d "$DOCS_ROOT" ] || fail "docs root not found at $DOCS_ROOT" \
    "pass DOCS_ROOT (or restore kask/docs) — an unscannable root fails loudly, never as a silent pass"

count=0
violations=0
while IFS= read -r f; do
    count=$((count + 1))
    rel=${f#"$root"/}
    if [ "$(sed -n '1p' "$f")" != "---" ]; then
        echo "FAIL: $rel does not open with a '---' frontmatter delimiter" >&2
        violations=$((violations + 1))
        continue
    fi
    if [ "$(awk 'NR>1 && /^---$/ {print "y"; exit}' "$f")" != "y" ]; then
        echo "FAIL: $rel frontmatter is not closed by a second '---' delimiter" >&2
        violations=$((violations + 1))
        continue
    fi
    # The frontmatter body: lines 2..(closing '---').
    fm=$(awk 'NR>1 && /^---$/ {exit} NR>1 {print}' "$f")
    for field in title audience last_updated version status domain mds_categories; do
        if ! printf '%s\n' "$fm" | grep -qE "^${field}:"; then
            echo "FAIL: $rel is missing the '$field' frontmatter field" >&2
            violations=$((violations + 1))
        fi
    done
done < <(find "$DOCS_ROOT" -type f -name '*.md' ! -path '*/archive/*')

if (( violations > 0 )); then
    fail "$violations frontmatter violation(s) across $count Markdown files (DOCUMENTATION_STANDARDS.md §2)"
fi
printf 'docs frontmatter gate: %s Markdown files carry the seven-field header\n' "$count"
