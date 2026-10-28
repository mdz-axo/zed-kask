#!/usr/bin/env bash
# Selftest for check-diagram-registry-parity.sh: the gate passes a balanced
# synthetic tree and fails when a Mermaid block carries no alignment record
# (the count-gate selftest's pattern — a gate that cannot fail is vacuous).
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
gate="$root/kask/scripts/check-diagram-registry-parity.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

mkdir -p "$tmp/docs"
cat > "$tmp/docs/registered.md" <<'EOF'
```mermaid
graph TD
    A --> B
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-TEST-001
verified_date: 2026-10-05
verified_against: selftest
status: VERIFIED
-->
EOF
cat > "$tmp/docs/DIAGRAMS_INDEX.md" <<'EOF'
# Index

| DIAGRAM_ID | Location | Verified date | Status |
| --- | --- | --- | --- |
| `DIAG-TEST-001` | registered.md | 2026-10-05 | VERIFIED |
EOF

DOCS_ROOT="$tmp/docs" bash "$gate" > /dev/null \
    || { echo "FAIL: parity gate rejected a balanced tree" >&2; exit 1; }

cat > "$tmp/docs/unregistered.md" <<'EOF'
```mermaid
graph TD
    B --> C
```
EOF

if DOCS_ROOT="$tmp/docs" bash "$gate" > /dev/null 2>&1; then
    echo "FAIL: parity gate accepted an unregistered mermaid block" >&2
    exit 1
fi

echo "diagram parity selftest: passes balance, fails on violation"
