#!/usr/bin/env bash
# CI gate: cargo clippy over the canonical kask package set — the same
# enumeration kask/scripts/test uses (every hkask-* crate, kask_bridge,
# dme-harness, plus the MCP servers from build/mcp-servers.txt), so the
# lint scope can never drift from the test scope.
#
# The gate enforces the WORKSPACE LINT PROFILE and nothing more: the
# deny-level lints pinned in the root [workspace.lints] (redundant_clone,
# dbg_macro, todo, disallowed_methods, …) fail the build; default-warn
# lints stay warnings. `-D warnings` is deliberately NOT passed — the
# workspace's own profile is the contract this gate checks.
#
# Why this gate exists: a redundant_clone deny-lint violation landed in
# committed code (hkask-mcp-research apa.rs:389, found by the 2026-11-16
# kask audit) with no gate red — clippy for the kask families was run by
# nobody: upstream's script/clippy covers the zed surface, pre-push stays
# seconds-tier by design, and the kask-invariants workflow had fmt + test
# but no clippy step. Minutes-tier belongs in the kask-tests CI job (the
# pre-push hook's documented split), which runs this script.
#
# Exit codes: 0 = every package clean under the profile, 1 = a clippy
# error (a deny lint fired, the build broke, or the package set failed to
# resolve — a scan failure is never a silent pass).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$REPO_ROOT"

# The MCP server binaries are the canonical install set; the glob below is
# belt-and-braces (every server package already matches hkask-*).
mapfile -t MCP_SERVERS < <(grep -vE '^\s*(#|$)' "$SCRIPT_DIR/build/mcp-servers.txt")

PACKAGES=(-p 'hkask-*' -p kask_bridge -p dme-harness)
for server in "${MCP_SERVERS[@]}"; do
  PACKAGES+=(-p "$server")
done

echo "kask/scripts/check-clippy: cargo clippy --locked ${PACKAGES[*]} (workspace lint profile)"
if ! cargo clippy --locked "${PACKAGES[@]}"; then
  echo "FAIL: cargo clippy red over the kask package set — a workspace deny lint fired or the build failed (see the diagnostics above)." >&2
  exit 1
fi
echo "OK: the workspace lint profile holds over every kask crate (${#MCP_SERVERS[@]} MCP servers, hkask-*, kask_bridge, dme-harness)."
