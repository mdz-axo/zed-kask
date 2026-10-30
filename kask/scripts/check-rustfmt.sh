#!/usr/bin/env bash
# Gate: the workspace is rustfmt-clean — `cargo fmt --all -- --check` exits 0.
#
# This is the CI check job's Formatting step
# (.github/workflows/kask-invariants.yml) moved to the pre-push point: fmt
# drift was the one invariant enforced only AFTER the push, so 18
# consecutive pushes went red on GitHub while every local gate stayed
# green (2026-10-29/30). The command is CI's exact command — a local
# variant would recreate the divergence class it exists to close.
#
# Check-only by design: it never mutates the working tree. A bare
# `cargo fmt --all` reformats every parallel stream's fmt-dirty in-flight
# files (the shared-tree trap, .rules) — format ONLY the files this gate
# names, never the whole tree.
#
# Self-tested via the pre-push hook selftest's fixture crate
# (kask/scripts/check-pre-push-selftest.sh, case 7c).

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || {
  echo "rustfmt gate: no git repo — cannot locate the workspace" >&2
  exit 1
}
cd "$REPO_ROOT"

if ! command -v cargo >/dev/null 2>&1; then
  echo "rustfmt gate: cargo not found on PATH — cannot check formatting (never a silent pass)" >&2
  exit 1
fi

out="$(cargo fmt --all -- --check 2>&1)" || {
  printf '%s\n' "$out" >&2
  echo "rustfmt gate: the files above are not rustfmt-clean (cargo fmt --all -- --check)." >&2
  echo "Format ONLY the listed files — a bare 'cargo fmt --all' reformats every parallel stream's in-flight files (.rules shared-tree trap)." >&2
  exit 1
}

echo "rustfmt gate: cargo fmt --all -- --check clean"