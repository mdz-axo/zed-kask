#!/usr/bin/env bash
# CI gate: pin the MCP-server build-profile seam (DIVERGENCE.md D46).
#
# The install CPU-burn defect: install.sh built the zed binary AND all 11
# MCP servers on the `release` profile (thin LTO + codegen-units=1), so
# every server crate pinned one core for minutes and an install pegged
# the whole machine. The fix has three parts that must drift together:
#
#   1. [profile.release-mcp] in the root Cargo.toml (lto=false,
#      codegen-units=16) — the cheap profile the servers build on.
#   2. install.sh builds servers with `--profile release-mcp` and the zed
#      binary with `--release`, with a `--jobs` cap (HKASK_BUILD_JOBS).
#   3. install.sh copies server binaries from target/release-mcp and the
#      zed binary from target/release.
#
# Any one of these regressing (e.g. an upstream rebase dropping the
# profile, or install.sh reverting to one `--release` invocation)
# reintroduces the burn silently. This check fails CI first.
#
# Usage: bash kask/scripts/build/check-build-profile.sh  (from repo root)
# Exit codes: 0 = seam intact, 1 = drift detected

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
CARGO_TOML="$ROOT/Cargo.toml"
INSTALL_SH="$ROOT/kask/scripts/build/install.sh"

fail() {
    echo "[FAIL] $1"
    exit 1
}

[ -f "$CARGO_TOML" ] || fail "Cargo.toml not found: $CARGO_TOML"
[ -f "$INSTALL_SH" ] || fail "install.sh not found: $INSTALL_SH"

# 1. The profile exists with the cheap settings.
grep -q '^\[profile\.release-mcp\]' "$CARGO_TOML" \
    || fail "root Cargo.toml lost [profile.release-mcp] (D46) — MCP servers would build on full release again"
grep -A5 '^\[profile\.release-mcp\]' "$CARGO_TOML" | grep -q 'lto = false' \
    || fail "[profile.release-mcp] must set lto = false (D46)"
grep -A5 '^\[profile\.release-mcp\]' "$CARGO_TOML" | grep -q 'codegen-units = 16' \
    || fail "[profile.release-mcp] must set codegen-units = 16 (D46)"

# 2. install.sh uses the split build with a jobs cap.
grep -q -- '--profile release-mcp' "$INSTALL_SH" \
    || fail "install.sh no longer builds MCP servers with --profile release-mcp (D46)"
grep -q -- '--jobs' "$INSTALL_SH" \
    || fail "install.sh lost its cargo --jobs cap — uncapped builds peg every core"
grep -q -- 'HKASK_BUILD_JOBS' "$INSTALL_SH" \
    || fail "install.sh lost the HKASK_BUILD_JOBS override"

# 3. install.sh reads the split output dirs.
grep -q 'target/release-mcp' "$INSTALL_SH" \
    || fail "install.sh no longer copies MCP servers from target/release-mcp (D46)"

# 4. The self-observing trace is wired (the observability half of D46).
grep -q 'build-monitor.sh' "$INSTALL_SH" \
    || fail "install.sh no longer starts build-monitor.sh — installs would burn CPU unobserved"

echo "[OK] build-profile seam intact: release-mcp profile, split install build, jobs cap, CPU trace"
