#!/usr/bin/env bash
# CI gate: pin the build-profile seams (DIVERGENCE.md D46, D52, D85).
#
# The install CPU-burn defect: install.sh built the zed binary AND all 10
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

# Overridable via env so the self-test can point at a fixture tree; the
# default preserves the production behavior exactly.
ROOT="${ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)}"
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

# The [profile.release] package tables — external dependencies must not
# inherit single-unit release codegen (D46; absorbs the retired D50), and
# zed's explicit codegen setting and thin LTO must stay preserved — are
# parsed, not grepped: owned by hkask-mcp-corpus/tests/build_profile.rs on
# the workspace `toml` dep (operator ruling 2026-10-13: zero Python —
# replaces the python3 tomllib heredoc landed 2026-09-08, e102d515e8a).

# 2. install.sh uses the split build with a jobs cap.
grep -q -- '--profile release-mcp' "$INSTALL_SH" \
    || fail "install.sh no longer builds MCP servers with --profile release-mcp (D46)"
grep -q -- '--jobs' "$INSTALL_SH" \
    || fail "install.sh lost its cargo --jobs cap — uncapped builds peg every core"
grep -q -- 'HKASK_BUILD_JOBS' "$INSTALL_SH" \
    || fail "install.sh lost the HKASK_BUILD_JOBS override"
# 2b. REMOVED 2026-09-30: the mimalloc default was a divergence upstream does
# not carry (upstream Linux builds run glibc malloc; mimalloc is an opt-in
# feature there). D85 landed it unmeasured on the performance side while the
# draw-cost regression (2.3–7.5 ms → 20–38 ms per draw) appeared only after
# it. The allocator divergence is removed for upstream parity; the glibc
# arena-ratchet memory finding (2026-09-28) is handled outside code if it
# recurs (MALLOC_ARENA_MAX in the launcher). mimalloc stays available as an
# opt-in feature, exactly as upstream.

# 3. install.sh reads the split output dirs.
grep -q 'target/release-mcp' "$INSTALL_SH" \
    || fail "install.sh no longer copies MCP servers from target/release-mcp (D46)"

# 3b. The installer offers release-class builds only (2026-10-02
# incident: a day of dev-profile daily-drivers installed via --debug read
# as a performance regression — two-symptom latency findings §7.9).
# --fast is the quick-rebuild path (parity flags), and no dev-profile
# route may return.
grep -q -- '--profile release-fast' "$INSTALL_SH" \
    || fail "install.sh lost the --fast zed build path (--profile release-fast)"
grep -q 'target/release-fast' "$INSTALL_SH" \
    || fail "install.sh must install the --fast zed binary from target/release-fast"
if grep -qE 'HKASK_BUILD_TYPE="debug"|target/debug' "$INSTALL_SH"; then
    fail "install.sh must not offer a dev-profile build route (the removed --debug footgun)"
fi

# 4. The self-observing trace is wired (the observability half of D46).
grep -q 'build-monitor.sh' "$INSTALL_SH" \
    || fail "install.sh no longer starts build-monitor.sh — installs would burn CPU unobserved"

# 5. The workspace-wide cargo jobs cap — bounds rust-analyzer flycheck,
# editor tasks, and agent builds that pass no explicit --jobs. The cap is 16
# (tuned for the 24-core dev box; smaller machines override CARGO_BUILD_JOBS).
grep -q '^jobs = 16' "$ROOT/.cargo/config.toml" \
    || fail ".cargo/config.toml lost its [build] jobs cap — uncapped cargo invocations (rust-analyzer flycheck, agent tasks) peg every core"

# 6. sccache wiring — install.sh and script/clippy must both detect the
# wrapper; a silent drop means installs and lint runs rebuild ~800 deps
# from scratch every time.
grep -q 'RUSTC_WRAPPER' "$INSTALL_SH" \
    || fail "install.sh lost its sccache RUSTC_WRAPPER wiring (D46)"
grep -q 'RUSTC_WRAPPER' "$ROOT/script/clippy" \
    || fail "script/clippy lost its sccache RUSTC_WRAPPER wiring (D46)"

# 7. The sccache wrapper must live OUTSIDE target/ — cargo clean deletes
# the entire target tree recursively, and on 2026-09-07 a clean deleted the
# wrapper so the next install ran uncached with a healthy 4 GiB disk cache
# (the silent-uncache defect). setup-sccache installs to
# ~/.local/lib/kask-sccache/ (HKASK_SCCACHE_DIR overrides); consumers must
# resolve it there, never inside target/. setup-sccache.ps1 is upstream
# Windows surface, unused by the Linux-only fork (D7), and is exempt.
for sccache_consumer in "$ROOT/script/setup-sccache" "$ROOT/script/clippy" "$INSTALL_SH"; do
    if grep -q 'target/sccache' "$sccache_consumer"; then
        fail "$sccache_consumer references target/sccache — cargo clean would delete the wrapper (D46 silent-uncache defect)"
    fi
done

# 8. REMOVED 2026-09-30 with the D85 mimalloc default (see 2b): the check
# pinned a divergence upstream does not carry. Upstream parity means no
# default allocator feature; mimalloc remains opt-in as upstream has it.

# Exercise the wrapper without Cargo: profile selection must not silently
# trigger a release dependency rebuild or weaken the lint coverage. A stub
# cargo records its argv; the captured arguments must match the expected
# clippy invocation exactly, and Cargo's failure status must propagate.
# Bash-native probe (operator ruling 2026-10-13: zero Python — replaces the
# python3 heredoc landed 2026-09-08, 5fd4bac424a).
probe_dir="$(mktemp -d)"
trap 'rm -rf "$probe_dir"' EXIT
stub_cargo="$probe_dir/cargo-stub"
arguments_file="$probe_dir/arguments"
printf '#!/bin/sh\nprintf "%%s\\n" "$@" > "$CLIPPY_ARGUMENTS"\nexit "${CLIPPY_STATUS:-0}"\n' > "$stub_cargo"
chmod 700 "$stub_cargo"

run_stubbed_wrapper() {
    # run_stubbed_wrapper <stub exit status> <wrapper arguments...>
    timeout 10 env CARGO="$stub_cargo" GITHUB_ACTIONS=true HKASK_BUILD_JOBS=8 \
        HKASK_SCCACHE_DIR="$probe_dir" CLIPPY_ARGUMENTS="$arguments_file" \
        CLIPPY_STATUS="$1" bash "$ROOT/script/clippy" "${@:2}" \
        >"$probe_dir/wrapper.log" 2>&1
}

expect_wrapper_args() {
    # expect_wrapper_args <expected scope words...> -- <wrapper arguments...>
    local -a scope=()
    while [ "$1" != "--" ]; do scope+=("$1"); shift; done
    shift
    rm -f "$arguments_file"
    local rc=0
    run_stubbed_wrapper 0 "$@" || rc=$?
    if [ "$rc" -ne 0 ]; then
        fail "clippy wrapper failed (rc=$rc): $(tail -n 5 "$probe_dir/wrapper.log" 2>/dev/null)"
    fi
    [ -f "$arguments_file" ] || fail "clippy wrapper never reached cargo (no arguments recorded)"
    local -a observed expected
    mapfile -t observed < "$arguments_file"
    expected=(clippy "$@" "${scope[@]}" "--jobs" 8 "--all-targets" "--all-features" "--" "--deny" "warnings")
    if [ "${observed[*]}" != "${expected[*]}" ]; then
        fail "clippy profile/coverage drift (D52): [${observed[*]}] != [${expected[*]}]"
    fi
}

expect_wrapper_args -- -p kask_bridge
expect_wrapper_args -- --release -p hkask-mcp
expect_wrapper_args -- --profile release-mcp --package hkask-mcp
expect_wrapper_args -- --profile=dev -p kask_bridge
# The bare workspace case must carry --exclude remote_server: under
# --all-features its debug-embed feature request unifies into the host
# rust-embed-impl and breaks dev-profile derives elsewhere in the graph
# (E0599). script/clippy appends the exclusion itself; this expectation
# pins it so it cannot silently regress. The -p/--package cases above pin
# the inverse — the exclusion must stay workspace-only.
expect_wrapper_args --workspace --exclude remote_server --

rc=0
run_stubbed_wrapper 42 -p hkask-mcp || rc=$?
if [ "$rc" -ne 42 ]; then
    fail "clippy wrapper must propagate Cargo's failure status (D52): rc=$rc"
fi

echo "[OK] build-profile seam intact: release-mcp profile, split install build, jobs cap, CPU trace"
