#!/bin/bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Overridable via env so the self-test can point at a fixture tree; the
# default preserves the production behavior exactly.
repo_root="${REPO_ROOT:-$(cd "$script_dir/../../.." && pwd)}"
errors=0

fail() {
    echo "FAIL: $1" >&2
    errors=$((errors + 1))
}

assert_absent() {
    [ ! -e "$1" ] || fail "forbidden upstream Zed packaging surface exists: $1"
}

assert_no_match() {
    if grep -Eq "$2" "$1"; then
        fail "$3: $1"
    fi
}

for path in \
    "$repo_root/crates/zed/resources/zed.desktop.in" \
    "$repo_root/crates/zed/resources/flatpak" \
    "$repo_root/crates/zed/resources/snap" \
    "$repo_root/script/flatpak" \
    "$repo_root/kask/scripts/build/zed-kask.desktop.in" \
    "$repo_root/.github/workflows/release.yml" \
    "$repo_root/.github/workflows/release_nightly.yml" \
    "$repo_root/.github/workflows/run_bundling.yml" \
    "$repo_root/crates/zed/resources/info" \
    "$repo_root/crates/zed/resources/windows" \
    "$repo_root/.github/actions/run_tests_windows" \
    "$repo_root/crates/auto_update" \
    "$repo_root/crates/auto_update_helper" \
    "$repo_root/crates/auto_update_ui"; do
    assert_absent "$path"
done

for script in bundle-linux bundle-mac snap-build; do
    if ! grep -q 'is disabled in zed-kask' "$repo_root/script/$script"; then
        fail "script/$script is not fail-closed"
    fi
done
if ! grep -q 'is disabled in zed-kask' "$repo_root/script/bundle-windows.ps1"; then
    fail "script/bundle-windows.ps1 is not fail-closed"
fi

# zed-kask: the auto_update, auto_update_helper, and auto_update_ui
# crates are deleted (D7). Updates are a source rebuild (script/install.sh);
# the download-only updater was removed 2026-10-02 (no releases published —
# every download path 404'd). The assertions above pin that the crates stay
# deleted; also reject reintroduced initialization or a simulated updater
# that could falsely claim an update was installed in the surviving host.
assert_absent "$repo_root/crates/title_bar/src/update_version.rs"
assert_no_match "$repo_root/crates/title_bar/src/title_bar.rs" \
    'SimulateUpdateAvailable|Restart to update Zed|update_version' \
    "title bar presents a simulated upstream Zed update"
assert_no_match "$repo_root/crates/zed/src/main.rs" 'auto_update::init|auto_update_ui::init' \
    "zed-kask initializes upstream Zed's updater"

# zed-kask is not distributed as a flatpak. Upstream's CLI flatpak module
# hard-codes upstream Zed's app ID (`dev.zed.Zed`) and binary layout, so a
# zed-kask CLI running inside upstream Zed's sandbox would re-exec and
# launch upstream Zed's editor. Similarly, honoring FLATPAK_XDG_* env vars
# in paths.rs would point zed-kask at upstream Zed's data directories.
# Both were removed under D7; these checks pin the removal against upstream
# merges re-introducing them.
assert_no_match "$repo_root/crates/cli/src/main.rs" 'mod flatpak|flatpak-spawn|FLATPAK_ID' \
    "zed-kask CLI contains upstream Zed's flatpak sandbox-escape code"
assert_no_match "$repo_root/crates/paths/src/paths.rs" 'FLATPAK_XDG_' \
    "zed-kask paths honor upstream Zed's flatpak XDG overrides"

# zed-kask: D7 workstation isolation (2026-09-01 incident). An upstream
# Flatpak Zed window opened on this workspace ran its rust-analyzer at
# 5.7 GB RSS / 263% CPU for 5 hours (`cargo check --workspace` over ~450
# crates) — the project-level `lsp` disable alone did not stop it, so the
# disable must exist in BOTH forms: the lsp section AND the per-language
# `language_servers` exclusion (older Zed builds honor only one). If either
# is removed, any editor that opens this repo re-burns the machine. These
# assertions pin both forms.
ZED_PROJECT_SETTINGS="$repo_root/.zed/settings.json"
if ! grep -q '"rust-analyzer"' "$ZED_PROJECT_SETTINGS" \
    || ! grep -q '"enabled": false' "$ZED_PROJECT_SETTINGS"; then
    fail ".zed/settings.json lost the lsp rust-analyzer disable (D7 workstation isolation)"
fi
if ! grep -q '"language_servers".*!rust-analyzer' "$ZED_PROJECT_SETTINGS" \
    && ! grep -A2 '"language_servers"' "$ZED_PROJECT_SETTINGS" | grep -q '!rust-analyzer'; then
    fail ".zed/settings.json lost the Rust language_servers '!rust-analyzer' exclusion (D7 workstation isolation)"
fi
assert_no_match "$repo_root/crates/zed/src/zed.rs" 'auto_update::|install_release_linux' \
    "zed-kask safe action reaches the upstream updater"
assert_no_match "$repo_root/crates/zed/src/zed/app_menus.rs" 'auto_update::Check|auto_update::UpdateZedKask' \
    "menu reaches the upstream updater"
assert_absent "$repo_root/kask/crates/kask_bridge/src/github_update.rs"

# zed-kask icons live in kask/assets/icons/ — the upstream icon files in
# crates/zed/resources/ must NOT exist. Leaving them creates a collision
# surface where upstream merge could restore upstream Zed icons and
# zed-kask code might accidentally reference them (commit dcc5aa6dd3).
for icon_path in \
    "$repo_root/crates/zed/resources/app-icon.png" \
    "$repo_root/crates/zed/resources/app-icon@2x.png" \
    "$repo_root/crates/zed/resources/app-icon-dev.png" \
    "$repo_root/crates/zed/resources/app-icon-dev@2x.png" \
    "$repo_root/crates/zed/resources/app-icon-nightly.png" \
    "$repo_root/crates/zed/resources/app-icon-nightly@2x.png" \
    "$repo_root/crates/zed/resources/app-icon-preview.png" \
    "$repo_root/crates/zed/resources/app-icon-preview@2x.png" \
    "$repo_root/crates/zed/resources/Document.icns" \
    "$repo_root/crates/zed/resources/windows/app-icon.ico" \
    "$repo_root/crates/zed/resources/windows/app-icon-dev.ico" \
    "$repo_root/crates/zed/resources/windows/app-icon-nightly.ico" \
    "$repo_root/crates/zed/resources/windows/app-icon-preview.ico"; do
    assert_absent "$icon_path"
done

# zed-kask code must reference icons from kask/assets/icons/, not crates/zed/resources/
for src_file in \
    "$repo_root/crates/zed/build.rs" \
    "$repo_root/crates/zed/src/zed.rs" \
    "$repo_root/crates/zed/src/visual_test_runner.rs" \
    "$repo_root/crates/windows_resources/src/windows_resources.rs" \
    "$repo_root/kask/scripts/build/install.sh" \
    "$repo_root/script/rasterize-zk-icon/src/main.rs"; do
    assert_no_match "$src_file" 'resources/app-icon|resources/windows/app-icon' \
        "zed-kask code references upstream Zed icon path instead of kask/assets/icons/"
done

assert_no_match "$repo_root/crates/zed/Cargo.toml" 'osx_url_schemes.*"zed"' \
    "crates/zed/Cargo.toml has osx_url_schemes = [\"zed\"] (must be absent or zed-kask)"

# nix/build.nix must not produce a binary named "zed", icons named "zed.png",
# .desktop files with upstream Zed app IDs, or declare mainProgram = "zed".
# The derivation is consumed by the devshell only; the installable binary is
# produced by kask/scripts/build/.
assert_no_match "$repo_root/nix/build.nix" '\$out/bin/zed[^-]|\$out/bin/zeditor|zed\.png|dev\.zed\.Zed|APP_CLI=.zed.|APP_ICON=.zed.|APP_NAME=.Zed' \
    "nix/build.nix installPhase produces colliding artifacts"
assert_no_match "$repo_root/nix/build.nix" 'mainProgram = "zed"' \
    "nix/build.nix declares mainProgram = \"zed\""
assert_no_match "$repo_root/nix/build.nix" 'zed\.dev' \
    "nix/build.nix references upstream Zed homepage/changelog"
assert_no_match "$repo_root/flake.nix" 'zed\.dev|Zed is a minimal' \
    "flake.nix references upstream Zed identity"

# nix/modules/packages.nix must not export a package that builds the zed derivation.
assert_no_match "$repo_root/nix/modules/packages.nix" 'packages\s*=\s*\{' \
    "nix/modules/packages.nix exports a nix package (zed-kask is not packaged via nix)"

# flake.nix must not reference upstream Zed's cachix.
assert_no_match "$repo_root/flake.nix" 'zed\.cachix' \
    "flake.nix references upstream Zed's cachix"

# Selftest seam: the installer-confinement suite below is exercised by the
# gate's own CI step on every run; the selftests set this to skip the
# redundant re-run. The fixture checks above have already run and their
# verdict is honored here — a violating fixture still fails. Loud by
# design: the skip is announced, never silent.
if [ -n "${ZED_ISOLATION_SKIP_INSTALLER_SUITE:-}" ]; then
    if [ "$errors" -gt 0 ]; then
        echo "REGRESSION: $errors Zed isolation violation(s) detected." >&2
        exit 1
    fi
    echo "SKIP: installer-confinement suite skipped (ZED_ISOLATION_SKIP_INSTALLER_SUITE set — selftest mode)"
    exit 0
fi

sandbox="$(mktemp -d)"
trap 'rm -rf "$sandbox"' EXIT
fake_home="$sandbox/home"
mkdir -p \
    "$fake_home/.local/bin" \
    "$fake_home/.local/zed.app/bin" \
    "$fake_home/.local/share/zed/threads" \
    "$fake_home/.local/share/applications" \
    "$fake_home/.config/zed" \
    "$sandbox/system-bin"
printf 'real-zed-binary\n' > "$fake_home/.local/zed.app/bin/zed"
printf 'real-zed-command\n' > "$fake_home/.local/bin/zed"
printf 'valuable-thread-data\n' > "$fake_home/.local/share/zed/threads/threads.db"
printf 'real-zed-settings\n' > "$fake_home/.config/zed/settings.json"
printf 'real-zed-launcher\n' > "$fake_home/.local/share/applications/dev.zed.Zed.desktop"
zed_before="$(sha256sum "$fake_home/.local/zed.app/bin/zed" "$fake_home/.local/bin/zed" "$fake_home/.local/share/zed/threads/threads.db" "$fake_home/.config/zed/settings.json" "$fake_home/.local/share/applications/dev.zed.Zed.desktop")"

export HOME="$fake_home"
export XDG_CONFIG_HOME="$fake_home/.config"
export SYSTEM_BIN="$sandbox/system-bin"
export BIN_DIR="$fake_home/.local/bin"
export INSTALL_DIR="$fake_home/.local"
export MCP_SERVERS_LIST_FILE="$script_dir/mcp-servers.txt"
# shellcheck source=install-common.sh
source "$script_dir/install-common.sh"

printf 'old-kask\n' > "$BIN_DIR/zed-kask"
printf 'old-server\n' > "$BIN_DIR/hkask-mcp-test"
prepare_install_dir
[ ! -e "$BIN_DIR/zed-kask" ] || fail "safe cleanup left the old zed-kask binary"
[ ! -e "$BIN_DIR/hkask-mcp-test" ] || fail "safe cleanup left an old MCP binary"

BIN_DIR="$fake_home/.local/zed.app/bin"
if prepare_install_dir >/dev/null 2>&1; then
    fail "installer accepted Zed's application bin directory"
fi
ln -s "$fake_home/.local/zed.app/bin" "$sandbox/aliased-bin"
BIN_DIR="$sandbox/aliased-bin"
if prepare_install_dir >/dev/null 2>&1; then
    fail "installer accepted a symlink into Zed's application bundle"
fi
if assert_kask_binary_destination "$fake_home/.local/bin/zed" >/dev/null 2>&1; then
    fail "installer accepted upstream Zed's command as a binary destination"
fi

zed_after="$(sha256sum "$fake_home/.local/zed.app/bin/zed" "$fake_home/.local/bin/zed" "$fake_home/.local/share/zed/threads/threads.db" "$fake_home/.config/zed/settings.json" "$fake_home/.local/share/applications/dev.zed.Zed.desktop")"
if [ "$zed_before" != "$zed_after" ]; then
    fail "installer confinement test modified a Zed-owned sentinel"
fi

# assert_not_zed_contaminated_env must block a build/install when the
# environment is coupled to the upstream Flatpak Zed (LD_LIBRARY_PATH pointing
# at a flatpak dev.zed lib dir). Pins the guard added to install.sh so the
# build can never silently couple to upstream Zed.
# (install-common.sh is sourced above, so the function is in scope.)
if LD_LIBRARY_PATH="/var/lib/flatpak/app/dev.zed.Zed/x86_64/stable/0000000000000000000000000000000000000000000000000000000000000000/files/lib" \
   assert_not_zed_contaminated_env "test" >/dev/null 2>&1; then
    fail "assert_not_zed_contaminated_env allowed a Flatpak-Zed-contaminated LD_LIBRARY_PATH"
fi

if [ "$errors" -gt 0 ]; then
    echo "REGRESSION: $errors Zed isolation violation(s) detected." >&2
    exit 1
fi

echo "PASS: zed-kask installer cannot write into upstream Zed-owned paths."
