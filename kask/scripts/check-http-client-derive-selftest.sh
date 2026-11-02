#!/usr/bin/env bash
# Self-test for the http_client derive gate (check-http-client-derive.sh):
# a bare `serde.workspace = true` fixture fails naming the fix; the D99
# line passes; a missing manifest fails loudly, never as a silent pass.
set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-http-client-derive.sh"
[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

failures=0

# Case 1: the upstream shape (bare workspace dep) fails, naming the fix.
printf '[dependencies]\nserde.workspace = true\nserde_json.workspace = true\n' > "$fixture/http_client.toml"
set +e; out="$(HTTP_CLIENT_MANIFEST="$fixture/http_client.toml" bash "$GATE" 2>&1)"; rc=$?; set -e
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -qF 'features = ["derive"]'; then
  echo "FAIL (case 1 — bare serde.workspace detected): rc=$rc"; printf '%s\n' "$out" | tail -4; failures=$((failures + 1))
else
  echo "OK (case 1 — bare serde.workspace detected, fix named)"
fi

# Case 2: the D99 line passes.
printf '[dependencies]\nserde = { workspace = true, features = ["derive"] }\nserde_json.workspace = true\n' > "$fixture/http_client.toml"
set +e; out="$(HTTP_CLIENT_MANIFEST="$fixture/http_client.toml" bash "$GATE" 2>&1)"; rc=$?; set -e
if [ "$rc" -ne 0 ]; then
  echo "FAIL (case 2 — the D99 line passes): expected 0, got $rc"; printf '%s\n' "$out" | tail -4; failures=$((failures + 1))
else
  echo "OK (case 2 — the D99 line passes)"
fi

# Case 3: a missing manifest fails loudly, never as a silent pass.
set +e; out="$(HTTP_CLIENT_MANIFEST="$fixture/absent.toml" bash "$GATE" 2>&1)"; rc=$?; set -e
if [ "$rc" -eq 0 ]; then
  echo "FAIL (case 3 — missing manifest): gate exited 0 on an absent manifest"; failures=$((failures + 1))
else
  echo "OK (case 3 — missing manifest fails loudly)"
fi

if (( failures > 0 )); then
  echo "check-http-client-derive-selftest: $failures case(s) failed" >&2
  exit 1
fi
echo "check-http-client-derive-selftest: all cases passed"