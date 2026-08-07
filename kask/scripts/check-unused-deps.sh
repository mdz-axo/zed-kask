#!/usr/bin/env bash
# Check for unused crate dependencies in kask crates using the nightly
# `unused_crate_dependencies` lint. `cargo machete` (the CI `deps` job)
# cannot detect crate-level unused deps — it only finds unused Cargo.toml
# entries, not deps that are declared but never imported in the lib target.
#
# This script catches the class that `cargo machete` misses: a dep in
# `[dependencies]` that the lib target never `use`s (e.g. `tokio` declared
# for the bin's `#[tokio::main]` but not used in the lib). Lib roots with
# a legitimate bin-needs-dep case carry `#![allow(unused_crate_dependencies)]`
# with an explanatory comment.
#
# Usage: bash scripts/check-unused-deps.sh
set -euo pipefail
cd "$(dirname "$0")/.."

echo "=== Unused crate dependencies (nightly, kask crates only) ==="

# Check if nightly is installed
if ! rustup run nightly rustc --version &>/dev/null; then
    echo "Installing nightly toolchain..."
    rustup toolchain install nightly --no-self-update
fi

# Build the list of kask crate -p flags (kask/crates/ + kask/mcp-servers/).
kask_crates=()
for dir in crates/hkask-* crates/kask_bridge mcp-servers/hkask-mcp-*; do
    if [ -f "$dir/Cargo.toml" ]; then
        name=$(grep '^name = ' "$dir/Cargo.toml" | head -1 | sed 's/name = "//;s/"//')
        kask_crates+=("-p" "$name")
    fi
done

# Check each kask crate's lib target. The lint fires on the lib target;
# bin targets that are thin `run().await` wrappers need no suppression.
# Lib roots with a legitimate bin-needs-dep case carry `#![allow(...)]`.
# Filter to only kask crate errors — the lint also fires on transitive
# dependencies (e.g. `perf`) which are not our concern.
errors=$(RUSTFLAGS="-D unused_crate_dependencies" \
    rustup run nightly cargo check --lib "${kask_crates[@]}" 2>&1 \
    | grep "^error: extern crate.*unused in crate 'hkask_\|error: extern crate.*unused in crate 'kask_" || true)

if [ -z "$errors" ]; then
    echo "OK: No unused crate dependencies in kask crates."
else
    echo ""
    echo "FAIL: Unused crate dependencies found:"
    echo "$errors"
    echo ""
    echo "Remove the unused dependencies from the crate's Cargo.toml,"
    echo "move test-only deps to [dev-dependencies], or add"
    echo "#![allow(unused_crate_dependencies)] to the lib root with a"
    echo "comment explaining why (e.g. bin-needs-dep case)."
    exit 1
fi
