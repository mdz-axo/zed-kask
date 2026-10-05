#!/usr/bin/env bash
# Diagram-registry parity gate (DOCUMENTATION_STANDARDS.md §4): every
# current-state Mermaid block under kask/docs carries exactly one
# DIAGRAM_ALIGNMENT record, and the DIAGRAMS_INDEX registry row count
# agrees — fences == records == index rows. The 2026-10-05 deep-alignment
# run found the compaction spec's diagram unregistered and the index prose
# miscounting (107 records vs 108 blocks); this gate makes that drift fail
# CI. The standards' §4.2 format example (`id: DIAG-<AREA>-<NNN>`) is the
# one sanctioned placeholder and is excluded from the record count.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
DOCS_ROOT=${DOCS_ROOT:-"$root/kask/docs"}
INDEX=${INDEX:-"$DOCS_ROOT/DIAGRAMS_INDEX.md"}

fail() {
    echo "FAIL: $1" >&2
    exit 1
}

[ -d "$DOCS_ROOT" ] || fail "docs root not found at $DOCS_ROOT"
[ -f "$INDEX" ] || fail "diagram index not found at $INDEX"

# `grep -c` exits 1 when nothing matches; the `|| true` guards keep that
# (an empty count is a valid reading, not an error) from aborting under
# `set -euo pipefail` — the awk END block still prints the sum.
fences=$(grep -rc '```mermaid' "$DOCS_ROOT" --include='*.md' | awk -F: '{s+=$2} END {print s+0}' || true)
id_lines=$(grep -rc '^id: DIAG-' "$DOCS_ROOT" --include='*.md' | awk -F: '{s+=$2} END {print s+0}' || true)
placeholders=$(grep -rc '^id: DIAG-<' "$DOCS_ROOT" --include='*.md' | awk -F: '{s+=$2} END {print s+0}' || true)
records=$((id_lines - placeholders))
rows=$(grep -c '^| `DIAG-' "$INDEX" || true)

[ "$fences" = "$records" ] || fail "mermaid fences ($fences) != DIAGRAM_ALIGNMENT records ($records; $id_lines id lines minus $placeholders format placeholders)"
[ "$fences" = "$rows" ] || fail "mermaid fences ($fences) != DIAGRAMS_INDEX rows ($rows)"
printf 'diagram parity gate: %s fences = %s records = %s index rows\n' "$fences" "$records" "$rows"
