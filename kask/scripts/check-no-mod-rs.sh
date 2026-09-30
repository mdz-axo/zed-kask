#!/usr/bin/env bash
# CI gate: no `mod.rs` files on the fork-owned surface — the fork uses the
# Rust 2018 `src/<module>.rs` layout (.rules, "Rust coding guidelines").
# Upstream crates are out of scope: they carry their own mod.rs files and
# the fork never edits them (DIVERGENCE.md).
#
# Exit codes: 0 = clean, 1 = mod.rs found on the fork surface (or the scan
# itself failed — never a silent pass).
set -euo pipefail
cd "$(dirname "$0")/.."

# Overridable via env so the self-test can point at a temp tree; the
# default preserves the production behavior exactly.
SCAN_ROOTS=( ${SCAN_ROOTS:-crates mcp-servers ../crates/hkask-*} )

set +e
found="$(find "${SCAN_ROOTS[@]}" -name mod.rs 2>/dev/null)"
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
    echo "FAIL: the mod.rs scan errored (find rc=$rc) — a scan failure is never a silent pass." >&2
    exit 1
fi
if [ -n "$found" ]; then
    printf '%s\n' "$found" | sed 's/^/FAIL: /' >&2
    echo "FAIL: mod.rs on the fork surface — use src/<module>.rs instead (.rules, Rust coding guidelines)." >&2
    exit 1
fi
echo "OK: no mod.rs files on the fork surface."
