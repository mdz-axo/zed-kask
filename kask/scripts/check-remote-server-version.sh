#!/usr/bin/env bash
# CI gate: pin the remote_server version-resolution seam (DIVERGENCE.md D56).
#
# D7 unifies the app version at the workspace level: crates/zed/Cargo.toml
# carries `version.workspace = true`. Upstream's remote_server build script
# parsed that manifest with the cargo_toml crate, whose Manifest rejects
# inherited values ("inherited workspace value"), failing every build
# touching the crate — cargo check -p remote_server, the full-workspace
# clippy (it builds as a dependency of collab/remote_connection), and any
# dev-profile --all-targets build of zed. The D56 fix resolves the version
# with plain toml parsing: zed's literal package.version when present
# (upstream layout), else the workspace root's [workspace.package].version.
#
# Any one of these regressing (an upstream rebase restoring the cargo_toml
# parse, the workspace package version disappearing, or the fallback being
# dropped) reintroduces the build failure silently. This check fails CI
# first.
#
# Usage: bash kask/scripts/check-remote-server-version.sh  (from repo root)
# Exit codes: 0 = seam intact, 1 = drift detected

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BUILD_RS="$ROOT/crates/remote_server/build.rs"
CARGO_TOML="$ROOT/Cargo.toml"

fail() {
    echo "[FAIL] $1"
    exit 1
}

[ -f "$BUILD_RS" ] || fail "remote_server build.rs not found: $BUILD_RS"
[ -f "$CARGO_TOML" ] || fail "root Cargo.toml not found: $CARGO_TOML"

# 1. The build script resolves the version via the D56 fallback, not the
#    cargo_toml Manifest parse that panics on inherited values.
grep -q 'fn zed_pkg_version' "$BUILD_RS" \
    || fail "remote_server build.rs lost zed_pkg_version() (D56) — the cargo_toml parse would panic again under D7"
grep -q 'WORKSPACE_MANIFEST' "$BUILD_RS" \
    || fail "remote_server build.rs lost the workspace-root fallback (D56)"
if grep -q 'cargo_toml::' "$BUILD_RS"; then
    fail "remote_server build.rs still parses with cargo_toml (D56 removed it)"
fi
if grep -q 'cargo_toml' "$ROOT/crates/remote_server/Cargo.toml"; then
    fail "remote_server Cargo.toml still declares the cargo_toml build-dependency (D56 removed it)"
fi

# 2. The workspace root declares the inherited package version the
#    fallback resolves.
grep -q '^\[workspace\.package\]' "$CARGO_TOML" \
    || fail "root Cargo.toml lost [workspace.package] — the D56 fallback has nothing to resolve"
grep -A10 '^\[workspace\.package\]' "$CARGO_TOML" | grep -q '^version *= *"' \
    || fail "root Cargo.toml [workspace.package] has no literal version — the D56 fallback cannot resolve the app version"

# 3. The real gate: the build script must run. This was the RED check
#    before the fix (build-script-build exit 101, "inherited workspace
#    value"). Bounded: check only, no tests, no --all-targets.
cd "$ROOT"
if ! cargo check -q -p remote_server >/dev/null 2>&1; then
    fail "cargo check -p remote_server failed — the D56 version resolution is broken (rerun without -q for the error)"
fi

echo "[OK] remote_server version resolution intact (D56): workspace package version present, fallback wired, build script runs"
