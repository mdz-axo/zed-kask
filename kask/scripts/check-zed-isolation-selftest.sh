#!/usr/bin/env bash
# Self-test for the zed-isolation gate (build/check-zed-isolation.sh).
#
# Pins the gate's failure mode against a fixture tree (the gate's REPO_ROOT
# override): a reintroduced upstream packaging surface must fail the gate,
# and a clean fixture must pass it. The installer-confinement section of
# the gate runs against the real install scripts (the gate's own
# $script_dir) in both cases.
#
# Exit codes: 0 = both cases behaved as expected, 1 = the gate is vacuous
# or broken.
set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/build/check-zed-isolation.sh"
[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

# A clean fixture: the .zed/settings.json the gate checks and the
# fail-closed bundler scripts it greps (missing files pass the absence
# and no-match checks by design; the bundler greps do not).
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

# The gate's D90 title-bar pin greps main.rs for the re-homed
# title_bar::init call (upstream chains it inside the deleted
# collab_ui::init). The clean fixture carries a minimal main.rs so the
# positive check has a file to read; case 3 below removes the call to pin
# the gate's failure mode.
write_clean_main_rs() {
  mkdir -p "$fixture/crates/zed/src"
  printf 'title_bar::init(cx);\n' > "$fixture/crates/zed/src/main.rs"
}
write_clean_main_rs

failures=0

# Case 1: a reintroduced upstream packaging surface fails the gate.
mkdir -p "$fixture/crates/zed/resources"
touch "$fixture/crates/zed/resources/zed.desktop.in"
set +e
out="$(REPO_ROOT="$fixture" ZED_ISOLATION_SKIP_INSTALLER_SUITE=1 bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ]; then
  echo "FAIL (case 1 — forbidden surface): gate exited 0 on a fixture carrying crates/zed/resources/zed.desktop.in"
  failures=$((failures + 1))
elif ! printf '%s\n' "$out" | grep -q "forbidden upstream Zed packaging surface exists"; then
  echo "FAIL (case 1 — forbidden surface): exit $rc but the violation was not identified"
  printf '%s\n' "$out" | tail -5
  failures=$((failures + 1))
else
  echo "OK (case 1 — forbidden surface): gate detected the reintroduced packaging surface"
fi

# Case 2: the clean fixture passes.
rm -rf "$fixture/crates"
write_clean_main_rs
set +e
out="$(REPO_ROOT="$fixture" ZED_ISOLATION_SKIP_INSTALLER_SUITE=1 bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -ne 0 ] || ! printf '%s\n' "$out" | grep -q "SKIP: installer-confinement suite skipped"; then
  echo "FAIL (case 2 — clean fixture): rc=$rc, expected exit 0 with an announced skip"
  printf '%s\n' "$out" | tail -5
  failures=$((failures + 1))
else
  echo "OK (case 2 — clean fixture): gate exited 0 with the skip announced"
fi

# Case 3: main.rs losing the re-homed title_bar::init fails the gate.
printf '// no title bar init here\n' > "$fixture/crates/zed/src/main.rs"
set +e
out="$(REPO_ROOT="$fixture" ZED_ISOLATION_SKIP_INSTALLER_SUITE=1 bash "$GATE" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ]; then
  echo "FAIL (case 3 — title bar init): gate exited 0 on a main.rs without title_bar::init"
  failures=$((failures + 1))
elif ! printf '%s\n' "$out" | grep -q "main.rs lost title_bar::init"; then
  echo "FAIL (case 3 — title bar init): exit $rc but the violation was not identified"
  printf '%s\n' "$out" | tail -5
  failures=$((failures + 1))
else
  echo "OK (case 3 — title bar init): gate detected the dropped title_bar::init"
fi

if [ "$failures" -eq 0 ]; then
  echo "SELFTEST OK: zed-isolation gate is alive (forbidden surface + clean fixture + title-bar init all pinned)"
  exit 0
fi
echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
exit 1
