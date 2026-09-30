#!/usr/bin/env bash
# Self-test for the desktop-collision gate (build/check-desktop-no-collision.sh).
#
# The gate is a 6-line exec wrapper onto check-zed-isolation.sh; this pins
# that the wrapper actually reaches the isolation checks (the environment
# passes through the exec): a fixture carrying a colliding upstream
# packaging surface fails through the wrapper, and a clean fixture passes.
set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/build/check-desktop-no-collision.sh"
[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/.zed" "$fixture/script"
cat > "$fixture/.zed/settings.json" <<'EOF'
{
  "lsp": { "rust-analyzer": { "enabled": false } },
  "languages": { "Rust": { "language_servers": ["!rust-analyzer"] } }
}
EOF
for s in bundle-linux bundle-mac snap-build bundle-windows.ps1; do
  cp "$KASK_ROOT/../script/$s" "$fixture/script/$s"
done

failures=0

# Case 1: a colliding surface fails through the wrapper.
mkdir -p "$fixture/crates/zed/resources"
touch "$fixture/crates/zed/resources/zed.desktop.in"
set +e
out="$(REPO_ROOT="$fixture" bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q "forbidden upstream Zed packaging surface exists"; then
  echo "FAIL (case 1 — violation through the wrapper): rc=$rc"
  printf '%s\n' "$out" | tail -5
  failures=1
else
  echo "OK (case 1 — violation through the wrapper detected)"
fi

# Case 2: the clean fixture passes through the wrapper.
rm -rf "$fixture/crates"
set +e
out="$(REPO_ROOT="$fixture" bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
  echo "FAIL (case 2 — clean fixture): expected 0, got $rc"
  printf '%s\n' "$out" | tail -5
  failures=1
else
  echo "OK (case 2 — clean fixture passes through the wrapper)"
fi

if [ "$failures" -eq 0 ]; then
  echo "SELFTEST OK: desktop-collision wrapper is alive"
  exit 0
fi
echo "SELFTEST FAIL: the wrapper did not behave as expected"
exit 1
