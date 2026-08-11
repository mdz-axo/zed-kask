#!/bin/bash
# Regression test: zed-kask uninstall paths.
#
# Pins three properties of uninstall_hkask (now in install-common.sh):
#
#   1. HKASK_REMOVE_CONFIG=true removes the REAL runtime dirs
#      ~/.config/zed-kask and ~/.local/share/zed-kask — NOT the stale,
#      nonexistent ~/.config/hkask / ~/.local/share/hkask. A prior version
#      targeted `hkask`, so HKASK_REMOVE_CONFIG silently no-op'd and left
#      config + data (db, threads, credentials) on disk while logging
#      "Removed config directory". This is the regression that motivated
#      moving the function into install-common.sh so it could be tested.
#
#   2. Without HKASK_REMOVE_CONFIG, config + data are PRESERVED (opt-in),
#      while binaries, the system symlink, the updater bundle, icons, the
#      desktop entry, shell PATH markers, and kask context_servers entries
#      are still removed.
#
#   3. install.sh's --uninstall dispatch does NOT call
#      assert_not_zed_contaminated_env. That guard prevents a *build/install*
#      from linking zed-kask against upstream Zed's bundled libs; --uninstall
#      does no linking (rm/sed/jq only), so applying the guard there would
#      block uninstall from Zed's own integrated terminal. The install) and
#      build-only) cases must STILL be guarded.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
install_sh="$script_dir/install.sh"
errors=0

fail() { echo "FAIL: $1" >&2; errors=$((errors + 1)); }

# --- sandbox -------------------------------------------------------------
sandbox="$(mktemp -d)"
trap 'rm -rf "$sandbox"' EXIT
fake_home="$sandbox/home"
install_prefix="$sandbox/install-prefix"
mkdir -p \
    "$fake_home/.local/bin" \
    "$install_prefix/bin" \
    "$install_prefix/share/zed-kask/install" \
    "$sandbox/system-bin"

export HOME="$fake_home"
export XDG_CONFIG_HOME="$fake_home/.config"
export XDG_DATA_HOME="$fake_home/.local/share"
export MCP_SERVERS_LIST_FILE="$script_dir/mcp-servers.txt"

# shellcheck source=install-common.sh
source "$script_dir/install-common.sh"

# install-common.sh hardcodes SYSTEM_BIN=/usr/local/bin at source time.
# Override to a writable sandbox dir so the symlink-removal path runs
# without touching the real /usr/local/bin.
SYSTEM_BIN="$sandbox/system-bin"
BIN_DIR="$install_prefix/bin"
INSTALL_DIR="$install_prefix"

# `sudo` is unavailable / undesirable in the test sandbox. Override it to
# fail so uninstall_hkask's `sudo rm ... || rm -f ...` falls through to the
# plain `rm -f` on the writable sandbox SYSTEM_BIN.
sudo() { return 1; }

# --- (re)build the installed state ---------------------------------------
seed_installed_state() {
    mkdir -p \
        "$SYSTEM_BIN" \
        "$BIN_DIR" \
        "$INSTALL_DIR/share/zed-kask/install" \
        "$XDG_CONFIG_HOME/zed-kask" \
        "$XDG_DATA_HOME/zed-kask/db" \
        "$XDG_DATA_HOME/zed-kask/threads" \
        "$XDG_DATA_HOME/icons/hicolor/512x512/apps" \
        "$XDG_DATA_HOME/icons/hicolor/1024x1024/apps" \
        "$XDG_DATA_HOME/applications" \
        "$fake_home"

    ln -sf "$BIN_DIR/zed-kask" "$SYSTEM_BIN/zed-kask"
    printf 'cli\n' > "$BIN_DIR/zed-kask"
    local server
    for server in "${MCP_SERVERS[@]}"; do
        printf 'mcp %s\n' "$server" > "$BIN_DIR/$server"
    done
    printf 'updater\n' > "$INSTALL_DIR/share/zed-kask/install/update-zed-kask.sh"
    printf 'icon\n' > "$XDG_DATA_HOME/icons/hicolor/512x512/apps/zed-kask.png"
    printf 'icon\n' > "$XDG_DATA_HOME/icons/hicolor/1024x1024/apps/dev.zed-kask.Zed-Kask.png"
    printf 'desktop\n' > "$XDG_DATA_HOME/applications/dev.zed-kask.Zed-Kask.desktop"
    printf 'creds\n' > "$XDG_CONFIG_HOME/zed-kask/development_credentials"
    printf 'settings\n' > "$XDG_CONFIG_HOME/zed-kask/settings.json"
    printf 'db\n' > "$XDG_DATA_HOME/zed-kask/db/db.sqlite"
    printf 'threads\n' > "$XDG_DATA_HOME/zed-kask/threads/threads.db"
    printf '# zed-kask PATH\nexport PATH="%s:$PATH"\n' "$BIN_DIR" > "$fake_home/.bashrc"
}

# --- Test 1: HKASK_REMOVE_CONFIG=true wipes zed-kask config + data -------
seed_installed_state
log1="$sandbox/uninstall1.log"
HKASK_REMOVE_CONFIG=true uninstall_hkask >"$log1" 2>&1 \
    || { cat "$log1"; fail "uninstall_hkask (HKASK_REMOVE_CONFIG=true) returned non-zero"; }

[ ! -L "$SYSTEM_BIN/zed-kask" ] || fail "system symlink survived uninstall"
[ ! -e "$BIN_DIR/zed-kask" ]    || fail "zed-kask binary survived uninstall"
for s in "${MCP_SERVERS[@]}"; do
    [ ! -e "$BIN_DIR/$s" ] || fail "MCP binary survived uninstall: $s"
done
[ ! -d "$INSTALL_DIR/share/zed-kask/install" ] || fail "updater bundle survived uninstall"
[ ! -f "$XDG_DATA_HOME/icons/hicolor/512x512/apps/zed-kask.png" ] || fail "icon survived uninstall"
[ ! -f "$XDG_DATA_HOME/applications/dev.zed-kask.Zed-Kask.desktop" ] || fail "desktop entry survived uninstall"
# The core regression: the REAL zed-kask dirs must be gone. If the path were
# `hkask` again, these survive and the test fails.
[ ! -e "$XDG_CONFIG_HOME/zed-kask/development_credentials" ] \
    || fail "config (credentials) survived HKASK_REMOVE_CONFIG=true — uninstall targeted the wrong dir (hkask vs zed-kask)"
[ ! -d "$XDG_CONFIG_HOME/zed-kask" ] || fail "config dir survived HKASK_REMOVE_CONFIG=true"
[ ! -d "$XDG_DATA_HOME/zed-kask" ]   || fail "data dir survived HKASK_REMOVE_CONFIG=true"
if grep -qE '# (zed-kask|hKask)' "$fake_home/.bashrc" 2>/dev/null; then
    fail "shell PATH marker survived uninstall"
fi

# --- Test 2: default preserves config + data (opt-in), still cleans rest --
seed_installed_state
# A kask context_server entry (must be removed) + a user one (must survive).
cat > "$XDG_CONFIG_HOME/zed-kask/settings.json" <<JSON
{
  "context_servers": {
    "kask-server": { "command": "$BIN_DIR/hkask-mcp-condenser" },
    "user-custom": { "command": "/usr/local/bin/something-else" }
  }
}
JSON

log2="$sandbox/uninstall2.log"
uninstall_hkask >"$log2" 2>&1 \
    || { cat "$log2"; fail "uninstall_hkask (default) returned non-zero"; }

[ ! -L "$SYSTEM_BIN/zed-kask" ] || fail "system symlink survived default uninstall"
[ ! -e "$BIN_DIR/zed-kask" ]    || fail "zed-kask binary survived default uninstall"
[ ! -d "$INSTALL_DIR/share/zed-kask/install" ] || fail "updater bundle survived default uninstall"
# config + data MUST survive — removal is opt-in.
[ -f "$XDG_CONFIG_HOME/zed-kask/development_credentials" ] \
    || fail "config was removed without HKASK_REMOVE_CONFIG (must be opt-in)"
[ -d "$XDG_CONFIG_HOME/zed-kask" ] || fail "config dir removed without HKASK_REMOVE_CONFIG (must be opt-in)"
[ -d "$XDG_DATA_HOME/zed-kask" ]   || fail "data dir removed without HKASK_REMOVE_CONFIG (must be opt-in)"
# settings.json: kask entry removed, user entry preserved.
kask_remaining=$(jq -r '.context_servers | has("kask-server")' "$XDG_CONFIG_HOME/zed-kask/settings.json")
user_kept=$(jq -r '.context_servers | has("user-custom")' "$XDG_CONFIG_HOME/zed-kask/settings.json")
[ "$kask_remaining" = "false" ] || fail "kask context_server entry survived uninstall"
[ "$user_kept" = "true" ]        || fail "non-kask context_server entry was removed by uninstall"

# --- Test 3 (static): install.sh dispatch guards -------------------------
uninstall_block=$(awk '/^[[:space:]]+uninstall\)/{f=1} f{print} f&&/^[[:space:]]+;;/{exit}' "$install_sh")
[ -n "$uninstall_block" ] || fail "could not locate uninstall) dispatch case in install.sh"
# Strip comment lines before grepping — the exemption comment legitimately
# names the guard to explain why it's absent, and must not count as a call.
uninstall_code=$(printf '%s\n' "$uninstall_block" | grep -vE '^[[:space:]]*#')
if printf '%s\n' "$uninstall_code" | grep -q 'assert_not_zed_contaminated_env'; then
    fail "uninstall) case calls assert_not_zed_contaminated_env — must be exempt (uninstall does no linking)"
fi
if ! printf '%s\n' "$uninstall_code" | grep -q 'uninstall_hkask'; then
    fail "uninstall) case does not call uninstall_hkask"
fi

install_block=$(awk '/^[[:space:]]+install\)/{f=1} f{print} f&&/^[[:space:]]+;;/{exit}' "$install_sh")
printf '%s\n' "$install_block" | grep -vE '^[[:space:]]*#' | grep -q 'assert_not_zed_contaminated_env' \
    || fail "install) case lost its assert_not_zed_contaminated_env guard"
build_block=$(awk '/^[[:space:]]+build-only\)/{f=1} f{print} f&&/^[[:space:]]+;;/{exit}' "$install_sh")
printf '%s\n' "$build_block" | grep -vE '^[[:space:]]*#' | grep -q 'assert_not_zed_contaminated_env' \
    || fail "build-only) case lost its assert_not_zed_contaminated_env guard"

if [ "$errors" -gt 0 ]; then
    echo "REGRESSION: $errors uninstall path failure(s) detected." >&2
    exit 1
fi

echo "PASS: zed-kask uninstall removes zed-kask (not hkask) config/data; config removal is opt-in; --uninstall is exempt from the build-coupling guard."