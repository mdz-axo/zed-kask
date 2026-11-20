#!/usr/bin/env bash
# Exercise check-clippy.sh against a fake `cargo` so every case is
# deterministic and needs no build: a clean clippy passes, clippy's own
# failure propagates, and the package flags the gate passes are the
# canonical set (hkask-*, kask_bridge, dme-harness, every server from
# build/mcp-servers.txt) — a gate whose package set goes vacuous is not a
# gate.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
gate="$repo_root/kask/scripts/check-clippy.sh"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

# Fake cargo: records its arguments for inspection, then exits $FAKE_EXIT.
cat > "$fixture/cargo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$FAKE_ARGS"
exit "${FAKE_EXIT:-0}"
EOF
chmod +x "$fixture/cargo"

run() { PATH="$fixture:$PATH" FAKE_ARGS="$fixture/args" FAKE_EXIT="$1" bash "$gate" >/dev/null 2>&1; }

expect() {
  local name=$1 want=$2 code=$3 got=0
  run "$code" || got=$?
  if (( got != want )); then
    echo "FAIL ($name): expected exit $want, got $got" >&2
    exit 1
  fi
  echo "OK ($name): exit $got"
}

expect "clean clippy passes" 0 0
expect "clippy failure propagates" 1 101

# Vacuity guard: the gate must ask cargo for clippy over the canonical
# package set — every server in build/mcp-servers.txt, the hkask-* glob,
# kask_bridge and dme-harness, with --locked.
PATH="$fixture:$PATH" FAKE_ARGS="$fixture/args" FAKE_EXIT=0 bash "$gate" >/dev/null 2>&1 || true
while IFS= read -r server; do
  case "$server" in ''|'#'*) continue ;; esac
  if ! grep -qxF -- "-p" "$fixture/args" || ! grep -qxF -- "$server" "$fixture/args"; then
    echo "FAIL (package set): $server missing from the clippy selection — the gate went vacuous." >&2
    exit 1
  fi
done < "$repo_root/kask/scripts/build/mcp-servers.txt"
for wanted in 'hkask-*' kask_bridge dme-harness clippy --locked; do
  if ! grep -qxF -- "$wanted" "$fixture/args"; then
    echo "FAIL (package set): $wanted missing from the clippy selection — the gate went vacuous." >&2
    exit 1
  fi
done
echo "OK (package set): clippy over hkask-*, kask_bridge, dme-harness and every MCP server."

echo 'OK: the clippy gate passes clean runs, propagates failures, and pins the canonical package set.'
