#!/usr/bin/env bash
# Selftest for check-docs-frontmatter.sh: the gate passes a conformant
# synthetic tree and fails one missing mandatory fields (the count-gate
# selftest's pattern — a gate that cannot fail is vacuous).
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
gate="$root/kask/scripts/check-docs-frontmatter.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/docs"
cat > "$tmp/docs/conformant.md" <<'EOF'
---
title: "Conformant"
audience: [developers]
last_updated: 2026-10-05
version: "1.0.0"
status: "Active"
domain: "Cross-cutting"
mds_categories: [domain]
---
Body text.
EOF

DOCS_ROOT="$tmp/docs" bash "$gate" > /dev/null \
    || { echo "FAIL: frontmatter gate rejected a conformant tree" >&2; exit 1; }

cat > "$tmp/docs/violating.md" <<'EOF'
---
title: "Violating"
audience: [developers]
---
Missing last_updated, version, status, domain, and mds_categories.
EOF

if DOCS_ROOT="$tmp/docs" bash "$gate" > /dev/null 2>&1; then
    echo "FAIL: frontmatter gate accepted a file missing five mandatory fields" >&2
    exit 1
fi

echo "docs frontmatter selftest: passes conformance, fails on violation"
