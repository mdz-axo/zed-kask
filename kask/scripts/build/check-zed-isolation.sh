#!/bin/bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
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
    "$repo_root/.github/workflows/run_bundling.yml"; do
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

assert_no_match "$repo_root/crates/zed/src/main.rs" 'auto_update::init|auto_update_ui::init' \
    "zed-kask initializes upstream Zed's updater"
assert_no_match "$repo_root/crates/zed/src/zed.rs" 'auto_update::|install_release_linux' \
    "zed-kask safe action reaches the upstream updater"
assert_no_match "$repo_root/crates/zed/src/zed/app_menus.rs" 'auto_update::Check|auto_update::UpdateZedKask' \
    "menu reaches the upstream updater"
assert_no_match "$repo_root/crates/auto_update/src/auto_update.rs" 'UpdateZedKask|poll_zed_kask|UpdateFeed::Github' \
    "removed zed-kask GitHub updater remains"
assert_absent "$repo_root/kask/crates/kask_bridge/src/github_update.rs"

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
# shellcheck source=install-binary.sh
source "$script_dir/install-binary.sh"

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

BIN_DIR="$fake_home/.local/bin"
payload="$sandbox/payload"
mkdir "$payload"
printf 'new zed-kask\n' > "$payload/zed-kask"
for server in "${MCP_SERVERS[@]}"; do
    printf 'new %s\n' "$server" > "$payload/$server"
done
archive="$sandbox/zed-kask-x86_64-unknown-linux-gnu.tar.gz"
tar -czf "$archive" -C "$payload" .
validate_archive "$archive" || fail "valid flat archive was rejected"
install_binaries "$payload" || fail "verified flat archive did not install"
[ "$(cat "$BIN_DIR/zed-kask")" = 'new zed-kask' ] || fail "safe installer did not replace zed-kask"
[ -x "$fake_home/.local/share/zed-kask/install/update-zed-kask.sh" ] || fail "safe updater bundle was not installed"

rollback_payload="$sandbox/rollback-payload"
cp -a "$payload" "$rollback_payload"
rm "$rollback_payload/${MCP_SERVERS[0]}"
printf 'known-good-before-failed-update\n' > "$BIN_DIR/zed-kask"
if install_binaries "$rollback_payload" >/dev/null 2>&1; then
    fail "installer accepted an incomplete release staging set"
fi
if [ "$(cat "$BIN_DIR/zed-kask")" != 'known-good-before-failed-update' ]; then
    fail "failed update modified an existing zed-kask binary"
fi

printf 'before-rename-failure\n' > "$BIN_DIR/zed-kask"
for server in "${MCP_SERVERS[@]}"; do
    printf 'before-rename-failure-%s\n' "$server" > "$BIN_DIR/$server"
done
failed_destination="$BIN_DIR/${MCP_SERVERS[0]}"
move_staged_binary() {
    if [ "$2" = "$failed_destination" ]; then
        return 1
    fi
    mv -f -- "$1" "$2"
}
if install_binaries "$payload" >/dev/null 2>&1; then
    fail "installer accepted a failed final binary replacement"
fi
unset -f move_staged_binary
if [ "$(cat "$BIN_DIR/zed-kask")" != 'before-rename-failure' ]; then
    fail "failed final replacement did not restore zed-kask"
fi
for server in "${MCP_SERVERS[@]}"; do
    if [ "$(cat "$BIN_DIR/$server")" != "before-rename-failure-$server" ]; then
        fail "failed final replacement did not restore $server"
    fi
done

for kind in zed zed_app traversal absolute unexpected symlink hardlink; do
    invalid="$sandbox/$kind.tar.gz"
    invalid_root="$sandbox/$kind"
    mkdir "$invalid_root"
    case "$kind" in
        zed) printf bad > "$invalid_root/zed"; tar -czf "$invalid" -C "$invalid_root" zed ;;
        zed_app) mkdir "$invalid_root/zed.app"; printf bad > "$invalid_root/zed.app/zed"; tar -czf "$invalid" -C "$invalid_root" zed.app ;;
        traversal) printf bad > "$invalid_root/zed-kask"; tar -czf "$invalid" --transform='s|^|../|' -C "$invalid_root" zed-kask ;;
        absolute) printf bad > "$invalid_root/zed-kask"; tar -P -czf "$invalid" --transform="s|^|$sandbox/absolute/|" -C "$invalid_root" zed-kask ;;
        unexpected) printf bad > "$invalid_root/not-kask"; tar -czf "$invalid" -C "$invalid_root" not-kask ;;
        symlink) ln -s zed-kask "$invalid_root/hkask-mcp-link"; tar -czf "$invalid" -C "$invalid_root" hkask-mcp-link ;;
        hardlink) printf bad > "$invalid_root/zed-kask"; ln "$invalid_root/zed-kask" "$invalid_root/hkask-mcp-link"; tar -czf "$invalid" -C "$invalid_root" zed-kask hkask-mcp-link ;;
    esac
    if validate_archive "$invalid" >/dev/null 2>&1; then
        fail "archive validator accepted $kind entry"
    fi
done

zed_after="$(sha256sum "$fake_home/.local/zed.app/bin/zed" "$fake_home/.local/bin/zed" "$fake_home/.local/share/zed/threads/threads.db" "$fake_home/.config/zed/settings.json" "$fake_home/.local/share/applications/dev.zed.Zed.desktop")"
if [ "$zed_before" != "$zed_after" ]; then
    fail "installer confinement test modified a Zed-owned sentinel"
fi

if [ "$errors" -gt 0 ]; then
    echo "REGRESSION: $errors Zed isolation violation(s) detected." >&2
    exit 1
fi

echo "PASS: zed-kask updater and installer cannot package, update, or write into upstream Zed-owned paths."
