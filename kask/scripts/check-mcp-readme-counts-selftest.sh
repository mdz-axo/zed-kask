#!/usr/bin/env bash
# check-mcp-readme-counts-selftest.sh — RED/GREEN proof for
# check-mcp-readme-counts.sh: a consistent fixture passes, and every
# drift class the gate exists for — catalog table row, Live pin sites
# count, dead pin-site citation, arithmetic total, pin name/argument
# divergence — blocks with a message naming the server and the surface.
# The known-positive discipline (the .rules zero-match instrument trap):
# a gate that cannot be shown red is a gate that silently rots green.
#
# Runs in CI via the kask/scripts/check-*-selftest.sh glob.
set -uo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
GATE="$HERE/check-mcp-readme-counts.sh"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

failures=0

expect() { # expect <description> <want_rc> <got_rc> <output-substring>
    local desc=$1 want=$2 got=$3 substr=$4
    if [ "$got" -eq "$want" ] && { [ -z "$substr" ] || printf '%s' "$OUT" | grep -q "$substr"; }; then
        echo "  ok: $desc"
    else
        echo "  FAIL: $desc (want rc=$want, got rc=$got)" >&2
        if [ -n "$substr" ] && ! printf '%s' "$OUT" | grep -q "$substr"; then
            echo "    expected output to contain: $substr" >&2
        fi
        failures=$((failures + 1))
    fi
}

build_fixture() {
    rm -rf "$WORK/fixture"
    mkdir -p "$WORK/fixture/servers/hkask-mcp-demo/src" "$WORK/fixture/list"
    cat > "$WORK/fixture/servers/hkask-mcp-demo/src/hkask_mcp_demo.rs" <<'RS'
//! Demo fixture server for the README-counts gate selftest.

hkask_mcp_server::tool_surface_pin!(
    DemoServer::combined_router(),
    "combined_router",
    7,
    tool_surface_is_exactly_7_registered_tools,
);
RS
    printf 'hkask-mcp-demo\n' > "$WORK/fixture/list/mcp-servers.txt"
    cat > "$WORK/fixture/readme.md" <<'MD'
# Demo registry

| Server | Crate | Purpose | Tools |
|--------|-------|---------|------:|
| Demo | `kask/mcp-servers/hkask-mcp-demo` | Demo | 7 |

Live pin sites: demo 7 (`hkask_mcp_demo.rs:7`).

Arithmetic: `7 = 7`.
MD
}

run_gate() {
    OUT=$(LIST="$WORK/fixture/list/mcp-servers.txt" \
        README="$WORK/fixture/readme.md" \
        SERVERS_DIR="$WORK/fixture/servers" \
        bash "$GATE" 2>&1)
    return $?
}

echo "Case 1: a consistent fixture passes."
build_fixture
run_gate; rc=$?
expect "consistent fixture passes" 0 "$rc" ""

echo "Case 2: a drifted catalog table row blocks."
build_fixture
sed -i 's/| Demo | 7 |/| Demo | 8 |/' "$WORK/fixture/readme.md"
run_gate; rc=$?
expect "drifted table row blocks, naming the server and surface" 1 "$rc" "catalog table row says 8 but the pin says 7"

echo "Case 3: a drifted Live pin sites count blocks."
build_fixture
sed -i 's/demo 7 (/demo 8 (/' "$WORK/fixture/readme.md"
run_gate; rc=$?
expect "drifted pin-site count blocks" 1 "$rc" "Live pin sites entry says 8 but the pin says 7"

echo "Case 4: a dead pin-site citation blocks."
build_fixture
sed -i 's/hkask_mcp_demo.rs:7/hkask_mcp_demo.rs:99/' "$WORK/fixture/readme.md"
run_gate; rc=$?
expect "dead citation blocks, naming the drift" 1 "$rc" "does not name the pin test"

echo "Case 5: a drifted arithmetic total blocks."
build_fixture
sed -i 's/`7 = 7`/`7 = 8`/' "$WORK/fixture/readme.md"
run_gate; rc=$?
expect "drifted arithmetic total blocks" 1 "$rc" "terms sum to 7, stated total 8"

echo "Case 6: pin name/argument divergence blocks."
build_fixture
sed -i 's/tool_surface_is_exactly_7_registered_tools/tool_surface_is_exactly_8_registered_tools/' \
    "$WORK/fixture/servers/hkask-mcp-demo/src/hkask_mcp_demo.rs"
run_gate; rc=$?
expect "name-arg divergence blocks, naming both numbers" 1 "$rc" "pin test name says 8 but the count argument says 7"

echo "Case 7: restoring consistency passes again."
build_fixture
run_gate; rc=$?
expect "restored fixture passes" 0 "$rc" ""

echo "----"
if [ "$failures" -eq 0 ]; then
    echo "SELFTEST OK: check-mcp-readme-counts blocks every drift class and passes consistency (7 cases)"
    exit 0
fi
echo "SELFTEST FAIL: $failures case(s) did not behave as expected." >&2
exit 1