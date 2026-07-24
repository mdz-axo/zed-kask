#!/usr/bin/env bash
# Pre-commit / pre-push check for unused crate dependencies.
#
# The CI nightly job enforces `-D unused_crate_dependencies`. Run this
# locally before pushing to catch unused deps before CI does.
#
# Usage: bash scripts/check-unused-deps.sh
set -euo pipefail
cd "$(dirname "$0")/.."

echo "=== Unused crate dependencies (nightly) ==="

# Check if nightly is installed
if ! rustup run nightly rustc --version &>/dev/null; then
    echo "Installing nightly toolchain..."
    rustup toolchain install nightly --no-self-update
fi

errors=$(RUSTFLAGS="-D unused_crate_dependencies" rustup run nightly cargo check --workspace 2>&1 | grep "^error" || true)

if [ -z "$errors" ]; then
    echo "OK: No unused crate dependencies."
else
    echo ""
    echo "FAIL: Unused crate dependencies found:"
    echo "$errors"
    echo ""
    echo "Remove the unused dependencies from the crate's Cargo.toml and re-run."
    exit 1
fi
