#!/bin/bash
# Regression test: staged binary publication (publish_binaries in
# install-common.sh).
#
# Pins the publication contract that replaced the delete-first sequence
# (prepare_install_dir unlinked the working installation before
# install_binary verified the replacements existed — a failure after
# deletion removed a working install):
#
#   1. A preflight failure (a built binary missing) leaves the working
#      installation fully intact — no deletions, no staged .new files.
#   2. A staging failure mid-inventory leaves the working installation
#      fully intact and cleans every staged .new file.
#   3. Strip status is truthful: a failing strip warns (naming the binary
#      and strip's own error) and never prints the success line; a
#      succeeding strip prints it. A strip failure never blocks
#      publication.
#   4. do_strip=false never invokes strip (release-fast keeps the full
#      debug symbols its profile promises).
#   5. A successful publish replaces every binary, sweeps stale hkask-mcp-*
#      servers not in the inventory, and works on a fresh (empty) BIN_DIR.
#   6. Publishing over a RUNNING binary does not disturb the running
#      process (rename semantics — the running process keeps its inode).
#
# publish_binaries' Zed-owned-path refusals are additionally pinned by
# check-zed-isolation.sh's installer-confinement suite.

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
errors=0

fail() { echo "FAIL: $1" >&2; errors=$((errors + 1)); }

# --- sandbox -------------------------------------------------------------
sandbox="$(mktemp -d)"
trap 'rm -rf "$sandbox"' EXIT
fake_home="$sandbox/home"
mkdir -p "$fake_home/.local/bin" "$fake_home/.local/zed.app/bin"

export HOME="$fake_home"
export XDG_CONFIG_HOME="$fake_home/.config"
export XDG_DATA_HOME="$fake_home/.local/share"
export MCP_SERVERS_LIST_FILE="$script_dir/mcp-servers.txt"

# shellcheck source=install-common.sh
source "$script_dir/install-common.sh"

BIN_DIR="$fake_home/.local/bin"

# --- fixture source tree: a built binary for every inventory name ---------
# (chmod +x: the preflight requires executable sources, like real cargo
# output — a mode-644 fixture would fail preflight for the wrong reason.)
src="$sandbox/src"
mkdir -p "$src"
printf 'new-kask\n' > "$src/zed-kask"
chmod +x "$src/zed-kask"
for server in "${MCP_SERVERS[@]}"; do
    printf 'new-server\n' > "$src/$server"
    chmod +x "$src/$server"
done

seed_installed() {
    rm -rf "$BIN_DIR"
    mkdir -p "$BIN_DIR"
    printf 'old-kask\n' > "$BIN_DIR/zed-kask"
    chmod +x "$BIN_DIR/zed-kask"
    local server
    for server in "${MCP_SERVERS[@]}"; do
        printf 'old-server\n' > "$BIN_DIR/$server"
        chmod +x "$BIN_DIR/$server"
    done
    printf 'stale-server\n' > "$BIN_DIR/hkask-mcp-stale-old-server"
    chmod +x "$BIN_DIR/hkask-mcp-stale-old-server"
}

assert_no_staged_files() {
    if [ -n "$(compgen -G "$BIN_DIR/*.new")" ]; then
        fail "staged .new file(s) left behind in $BIN_DIR"
    fi
}

assert_old_intact() {
    [ "$(cat "$BIN_DIR/zed-kask")" = "old-kask" ] \
        || fail "working zed-kask binary was disturbed"
    local server
    for server in "${MCP_SERVERS[@]}" hkask-mcp-stale-old-server; do
        [ -f "$BIN_DIR/$server" ] || fail "working MCP binary was removed: $server"
    done
}

# --- Test 1: preflight failure preserves the working installation ---------
seed_installed
rm -f "$src/zed-kask"
if publish_binaries "$src" "$src" true >"$sandbox/t1.log" 2>&1; then
    fail "publish_binaries succeeded with a missing built binary"
fi
grep -q "Built binary not found" "$sandbox/t1.log" \
    || fail "preflight failure was not reported loudly"
assert_old_intact
assert_no_staged_files
printf 'new-kask\n' > "$src/zed-kask"
chmod +x "$src/zed-kask"

# --- Test 2: staging failure mid-inventory preserves the working install --
seed_installed
cp_fail_dest="$BIN_DIR/${MCP_SERVERS[0]}.new"
# shellcheck disable=SC2329 -- invoked indirectly by publish_binaries
cp() {
    if [ "$2" = "$cp_fail_dest" ]; then
        echo "injected cp failure" >&2
        return 1
    fi
    command cp "$@"
}
if publish_binaries "$src" "$src" true >"$sandbox/t2.log" 2>&1; then
    fail "publish_binaries succeeded despite a staging failure"
fi
grep -q "Failed to stage" "$sandbox/t2.log" \
    || fail "staging failure was not reported loudly"
assert_old_intact
assert_no_staged_files
unset -f cp

# --- Test 3: strip status is truthful --------------------------------------
seed_installed
strip_fail_dir="$sandbox/strip-fail"
mkdir -p "$strip_fail_dir"
printf '#!/bin/sh\necho "injected strip failure" >&2\nexit 1\n' > "$strip_fail_dir/strip"
chmod +x "$strip_fail_dir/strip"
if ! PATH="$strip_fail_dir:$PATH" publish_binaries "$src" "$src" true >"$sandbox/t3.log" 2>&1; then
    fail "publish_binaries must survive a strip failure (non-fatal)"
fi
grep -q "strip failed on zed-kask" "$sandbox/t3.log" \
    || fail "strip failure was not warned about"
grep -q "injected strip failure" "$sandbox/t3.log" \
    || fail "strip's own error was discarded"
if grep -q "Stripped debug symbols from zed-kask" "$sandbox/t3.log"; then
    fail "strip success line printed despite strip failing"
fi
[ "$(cat "$BIN_DIR/zed-kask")" = "new-kask" ] \
    || fail "strip failure prevented publication"

strip_ok_dir="$sandbox/strip-ok"
mkdir -p "$strip_ok_dir"
printf '#!/bin/sh\nexit 0\n' > "$strip_ok_dir/strip"
chmod +x "$strip_ok_dir/strip"
seed_installed
if ! PATH="$strip_ok_dir:$PATH" publish_binaries "$src" "$src" true >"$sandbox/t3b.log" 2>&1; then
    fail "publish_binaries failed with a succeeding strip"
fi
grep -q "Stripped debug symbols from zed-kask" "$sandbox/t3b.log" \
    || fail "strip success was not reported"

# --- Test 4: do_strip=false never invokes strip ----------------------------
seed_installed
if ! PATH="$strip_fail_dir:$PATH" publish_binaries "$src" "$src" false >"$sandbox/t4.log" 2>&1; then
    fail "publish_binaries failed with do_strip=false"
fi
if grep -q "strip failed on" "$sandbox/t4.log"; then
    fail "strip was invoked despite do_strip=false"
fi
grep -q "Keeping debug symbols" "$sandbox/t4.log" \
    || fail "symbols-kept notice missing"

# --- Test 5: successful publish replaces, sweeps, and handles fresh dirs ---
seed_installed
if ! publish_binaries "$src" "$src" false >"$sandbox/t5.log" 2>&1; then
    fail "publish_binaries failed on a clean fixture"
fi
[ "$(cat "$BIN_DIR/zed-kask")" = "new-kask" ] \
    || fail "publish left the old zed-kask binary in place"
[ ! -e "$BIN_DIR/hkask-mcp-stale-old-server" ] \
    || fail "publish left a stale (non-inventory) MCP binary"
for server in "${MCP_SERVERS[@]}"; do
    [ "$(cat "$BIN_DIR/$server")" = "new-server" ] \
        || fail "publish did not replace MCP server: $server"
done
grep -q "Removed stale MCP server binary: hkask-mcp-stale-old-server" "$sandbox/t5.log" \
    || fail "stale sweep was not logged"

rm -rf "$BIN_DIR"
mkdir -p "$BIN_DIR"
if ! publish_binaries "$src" "$src" false >"$sandbox/t5b.log" 2>&1; then
    fail "publish_binaries failed on a fresh (empty) install dir"
fi
[ "$(cat "$BIN_DIR/zed-kask")" = "new-kask" ] \
    || fail "fresh install missing zed-kask"

# --- Test 6: publishing over a RUNNING binary keeps the process alive ------
seed_installed
runner="$BIN_DIR/${MCP_SERVERS[0]}"
cat > "$runner" <<'EOF'
#!/bin/bash
while :; do sleep 1; done
EOF
chmod +x "$runner"
"$runner" &
runner_pid=$!
sleep 0.2
if ! publish_binaries "$src" "$src" false >"$sandbox/t6.log" 2>&1; then
    fail "publish_binaries failed while the target binary was running"
fi
if ! kill -0 "$runner_pid" 2>/dev/null; then
    fail "publishing over a running binary killed the running process"
fi
[ "$(cat "$runner")" = "new-server" ] \
    || fail "publish did not replace the running binary's file"
kill "$runner_pid" 2>/dev/null || true
wait "$runner_pid" 2>/dev/null || true

if [ "$errors" -gt 0 ]; then
    echo "REGRESSION: $errors staged-publication failure(s) detected." >&2
    exit 1
fi

echo "PASS: staged publication preserves the working installation on failure; strip status is truthful."
