#!/bin/bash
# Stamp the version from VERSION into all files that carry a version number.
#
# VERSION  — single source of truth (user-controlled, checked into git)
# CI runs this on every push. It OVERWRITES whatever version number
# an agent may have hallucinated into Cargo.toml, install.sh, or elsewhere.
#
# Usage:  bash scripts/ci/stamp-version.sh
set -euo pipefail

cd "$(dirname "$0")/../.."

if [ ! -f VERSION ]; then
    echo "ERROR: VERSION file missing — create it with the project version (e.g., 0.31.0)"
    exit 1
fi

V=$(head -1 VERSION | tr -d '[:space:]')
if ! echo "$V" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "ERROR: VERSION must be semver (e.g., 0.31.0), got: $V"
    exit 1
fi

echo "Version: $V  (from VERSION)"

# ── Stamp: Cargo.toml ──────────────────────────────────────────────────────
sed -i "s/^version = \"[0-9]*\.[0-9]*\.[0-9]*\"/version = \"$V\"/" Cargo.toml
echo "  Cargo.toml → $V"

# ── Stamp: install.sh ──────────────────────────────────────────────────────
sed -i "s/HKASK_VERSION=\"\${HKASK_VERSION:-[0-9]*\.[0-9]*\.[0-9]*}\"/HKASK_VERSION=\"\${HKASK_VERSION:-${V}}\"/" scripts/build/install.sh
echo "  scripts/build/install.sh → $V"

echo "Stamp complete: $V"
