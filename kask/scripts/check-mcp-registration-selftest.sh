#!/usr/bin/env bash
# Self-test for the registration-pin gate (check-mcp-registration.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before
# its status: enforced is trusted", and pins the specific weakness this gate
# shipped with: detection was `grep -rqn "tool_surface_pin!"`, which a
# COMMENTED-OUT pin satisfies. Commenting out the pin is the cheapest way to
# disable it, so the gate was green on exactly the change it exists to catch.
# Three in-tree servers also carry prose mentioning the macro above their real
# pin, so "mentions the name" was never equivalent to "carries the pin".
#
# Cases below are a matched set: every bypass shape must FAIL, and every real
# invocation shape accepted by Rust must PASS. A detector that rejects bypasses
# by rejecting everything is not hardened, it is broken — the positive cases
# are the control half that catches that. The gate must use the repository's
# pinned Rust parser: legal whitespace before `!` and all three macro
# delimiters are syntax, while comments and strings are not macro invocations.
#
# Design: a synthetic server tree under a temp dir, with LIST and SERVERS_DIR
# pointed at it. Each case writes one source shape and asserts the gate's exit
# code. No repository file is touched.
#
# Exit codes:
#   0 — every bypass detected AND every real shape still accepted
#   1 — at least one case behaved wrongly (gate is vacuous or over-strict)
#
# Usage: bash scripts/check-mcp-registration-selftest.sh

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-mcp-registration.sh"

if [ ! -f "$GATE" ]; then
  echo "FAIL: gate not found at $GATE"
  exit 1
fi

TMPDIR="$(mktemp -d)"
cleanup() { rm -rf "$TMPDIR"; }
trap cleanup EXIT

PKG="hkask-mcp-selftest"
SRC="$TMPDIR/servers/$PKG/src"
mkdir -p "$SRC"
printf '%s\n' "$PKG" > "$TMPDIR/mcp-servers.txt"

failures=0

# run_case <label> <expected_rc> <source-text>
run_case() {
  local label="$1" expected="$2" body="$3" rc out
  printf '%s\n' "$body" > "$SRC/lib.rs"
  set +e
  out=$(LIST="$TMPDIR/mcp-servers.txt" SERVERS_DIR="$TMPDIR/servers" bash "$GATE" 2>&1)
  rc=$?
  set -e
  if [ "$rc" -ne "$expected" ]; then
    echo "FAIL ($label): expected exit $expected, got $rc"
    printf '%s\n' "$out" | sed 's/^/    /'
    failures=$((failures + 1))
  else
    echo "OK ($label): exit $rc as expected"
  fi
}

# ── Bypass shapes: the pin is NOT in force; the gate must exit 1. ──────────

run_case "case 1 — commented-out pin" 1 \
'// hkask_mcp_server::tool_surface_pin!(SERVER, TOOL_NAMES);'

run_case "case 2 — pin inside a string literal" 1 \
'fn doc() { let s = "hkask_mcp_server::tool_surface_pin!(X)"; let _ = s; }'

run_case "case 3 — pin inside a block comment" 1 \
'/* temporarily disabled:
hkask_mcp_server::tool_surface_pin!(SERVER, TOOL_NAMES);
*/'

run_case "case 4 — prose mention only (no invocation)" 1 \
'// Mirrors the shared tool_surface_pin! macro every built-in server carries.
fn main() {}'

run_case "case 5 — pin nested in an unconditionally disabled module" 1 \
'#[cfg(any())]
mod disabled {
    tool_surface_pin!(SERVER, TOOL_NAMES);
}'

# ── Real shapes: Rust accepts each one; the gate must exit 0. ─────────────

run_case "case 6 — qualified multi-line invocation" 0 \
'    hkask_mcp_server::tool_surface_pin!(
        SERVER, TOOL_NAMES
    );'

run_case "case 7 — bare unqualified invocation" 0 \
'tool_surface_pin!(SERVER, TOOL_NAMES);'

run_case "case 8 — brace-delimited invocation" 0 \
'hkask_mcp_server::tool_surface_pin! {
    SERVER
}'

run_case "case 9 — qualified invocation with legal whitespace before bang" 0 \
'hkask_mcp_server::tool_surface_pin ! (
    SERVER, TOOL_NAMES
);'

run_case "case 10 — bracket-delimited invocation" 0 \
'tool_surface_pin![SERVER, TOOL_NAMES]'

run_case "case 11 — real pin below a prose mention" 0 \
'// the shared `tool_surface_pin!` macro every built-in server carries.
    hkask_mcp_server::tool_surface_pin!(
        SERVER, TOOL_NAMES
    );'

# ── Structural control: a listed server with no directory must fail. ──────

rm -rf "$TMPDIR/servers/$PKG"
set +e
MISSING_OUT=$(LIST="$TMPDIR/mcp-servers.txt" SERVERS_DIR="$TMPDIR/servers" bash "$GATE" 2>&1)
MISSING_RC=$?
set -e
if [ "$MISSING_RC" -ne 1 ]; then
  echo "FAIL (case 12 — listed server absent from disk): expected exit 1, got $MISSING_RC"
  printf '%s\n' "$MISSING_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 12 — listed server absent from disk): exit 1 as expected"
fi

if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: registration-pin gate detects disabled pins and still accepts every real pin shape"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) behaved wrongly"
  echo "See kask/scripts/check-mcp-registration.sh."
  exit 1
fi
