#!/usr/bin/env bash
# CI gate: pin the remote_server version-resolution seam (DIVERGENCE.md D56).
#
# D7 unifies the app version at the workspace level: crates/zed/Cargo.toml
# carries `version.workspace = true`. Three build scripts and one xtask CI
# step read that manifest expecting a literal version — the cargo_toml
# Manifest parse panics ("inherited workspace value"), the line-scans find
# nothing ("Version not found") — failing every build touching those crates
# and the scheduled compliance check. The D56 fix resolves the version from
# the literal when present (upstream layout), else from the workspace root's
# [workspace.package].version (the D7 layout).
#
# Any one of these regressing (an upstream rebase restoring the old parses,
# the workspace package version disappearing, or a fallback being dropped)
# reintroduces the failures silently. This check fails CI first.
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

# 1. remote_server resolves the version via the D56 fallback, not the
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

# 2. The CLI build scripts carry the same fallback (they line-scanned for a
#    literal that D7 removed).
for script in eval_cli edit_prediction_cli; do
    BUILD="$ROOT/crates/$script/build.rs"
    [ -f "$BUILD" ] || fail "$script build.rs not found: $BUILD"
    grep -q 'fn zed_pkg_version' "$BUILD" \
        || fail "$script build.rs lost zed_pkg_version() (D56) — the line scan would fail again under D7"
    grep -q '\[workspace.package\]' "$BUILD" \
        || fail "$script build.rs lost the workspace-root fallback (D56)"
done

# 3. The xtask compliance step resolves the version under D7 too.
grep -q 'workspace\.package' "$ROOT/tooling/xtask/src/tasks/workflows/compliance_check.rs" \
    || fail "xtask compliance_check lost the workspace-version fallback (D56) — the scheduled check would fail under D7"

# 4. The workspace root declares the inherited package version the
#    fallbacks resolve.
grep -q '^\[workspace\.package\]' "$CARGO_TOML" \
    || fail "root Cargo.toml lost [workspace.package] — the D56 fallbacks have nothing to resolve"
grep -A10 '^\[workspace\.package\]' "$CARGO_TOML" | grep -q '^version *= *"' \
    || fail "root Cargo.toml [workspace.package] has no literal version — the D56 fallbacks cannot resolve the app version"

# 5. The real gate: the build scripts must run. These were the RED checks
#    before the fix (build-script-build exit 101). Bounded: check only, no
#    tests, no --all-targets.
cd "$ROOT"
for package in remote_server eval_cli edit_prediction_cli; do
    if ! cargo check -q -p "$package" >/dev/null 2>&1; then
        fail "cargo check -p $package failed — the D56 version resolution is broken (rerun without -q for the error)"
    fi
done

echo "[OK] remote_server version resolution intact (D56): workspace package version present, fallbacks wired in remote_server + eval_cli + edit_prediction_cli + xtask compliance, build scripts run"
