#!/usr/bin/env bash
# CI gate: pin the build-profile seams (DIVERGENCE.md D46, D52, D85).
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

# External dependencies must not inherit single-unit release codegen (D46;
# absorbs the retired D50, which explicitly extended D46).
python3 - "$CARGO_TOML" <<'PY'
import sys
import tomllib

with open(sys.argv[1], "rb") as source:
    release = tomllib.load(source)["profile"]["release"]
packages = release.get("package", {})
if packages.get("*", {}).get("codegen-units") != 16:
    sys.exit("[FAIL] release external dependencies must use codegen-units = 16 (D46)")
if packages.get("zed", {}).get("codegen-units") != 16 or release.get("lto") != "thin":
    sys.exit("[FAIL] D46 must preserve zed's explicit codegen setting and thin LTO")
PY

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

# 3b. The --fast path keeps parity flags and --debug warns about its costs
# (2026-10-02 incident: a day of dev-profile daily-drivers installed via
# --debug read as a performance regression — two-symptom latency findings
# §7.9). A silent revert to dev flags, or a dropped warning, reintroduces
# the class.
grep -q -- '--profile release-fast' "$INSTALL_SH" \
    || fail "install.sh lost the --fast zed build path (--profile release-fast)"
grep -q 'target/release-fast' "$INSTALL_SH" \
    || fail "install.sh must install the --fast zed binary from target/release-fast"
grep -q 'debug_assertions' "$INSTALL_SH" \
    || fail "install.sh --debug must warn about the dev-profile costs (debug_assertions tax, blind hang detector)"

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
# trigger a release dependency rebuild or weaken the lint coverage.
python3 - "$ROOT/script/clippy" <<'PY'
import os
from pathlib import Path
import subprocess
import sys
import tempfile

wrapper = Path(sys.argv[1])
with tempfile.TemporaryDirectory(prefix="kask-clippy-profile-") as directory:
    directory = Path(directory)
    arguments_file = directory / "arguments"
    cargo = directory / "cargo-stub"
    cargo.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$CLIPPY_ARGUMENTS"\nexit "${CLIPPY_STATUS:-0}"\n')
    cargo.chmod(0o700)
    environment = dict(os.environ, CARGO=str(cargo), GITHUB_ACTIONS="true",
                       HKASK_BUILD_JOBS="8", HKASK_SCCACHE_DIR=str(directory),
                       CLIPPY_ARGUMENTS=str(arguments_file), CLIPPY_STATUS="0")
    cases = [
        (["-p", "kask_bridge"], []),
        (["--release", "-p", "hkask-mcp"], []),
        (["--profile", "release-mcp", "--package", "hkask-mcp"], []),
        (["--profile=dev", "-p", "kask_bridge"], []),
        # The bare workspace case must carry --exclude remote_server: under
        # --all-features its debug-embed feature request unifies into the
        # host rust-embed-impl and breaks dev-profile derives elsewhere in
        # the graph (E0599). script/clippy appends the exclusion itself;
        # this expectation pins it so it cannot silently regress. The
        # -p/--package cases above pin the inverse — the exclusion must
        # stay workspace-only.
        ([], ["--workspace", "--exclude", "remote_server"]),
    ]
    for arguments, scope in cases:
        result = subprocess.run(["bash", str(wrapper), *arguments], env=environment,
                                capture_output=True, text=True, timeout=10)
        if result.returncode != 0:
            sys.exit(f"[FAIL] clippy wrapper failed: {result.stderr}")
        observed = arguments_file.read_text().splitlines()
        expected = ["clippy", *arguments, *scope, "--jobs", "8", "--all-targets",
                    "--all-features", "--", "--deny", "warnings"]
        if observed != expected:
            sys.exit(f"[FAIL] clippy profile/coverage drift (D52): {observed!r}; expected {expected!r}")
    environment["CLIPPY_STATUS"] = "42"
    result = subprocess.run(["bash", str(wrapper), "-p", "hkask-mcp"], env=environment,
                            capture_output=True, text=True, timeout=10)
    if result.returncode != 42:
        sys.exit("[FAIL] clippy wrapper must propagate Cargo's failure status (D52)")
print("[OK] clippy wrapper: dev default, explicit profiles, lint coverage, failure propagation")
PY

echo "[OK] build-profile seam intact: release-mcp profile, split install build, jobs cap, CPU trace"
