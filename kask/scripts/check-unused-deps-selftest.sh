#!/usr/bin/env bash
# Self-test for check-unused-deps.sh (S230-F01).
#
# Verifies on a zero-network local path-dependency workspace:
#   1. An unused library dependency fails closed (exit 1) naming the unused dep.
#   2. Using the dependency exits 0.
#   3. Empty KASK_UNUSED_DEPS_DIRS fails closed (exit 1) rather than passing 0=0.
#   4. A compile error fails closed (exit 1) rather than reading as 0 unused deps.

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-unused-deps.sh"
[ -f "$GATE" ] || { echo "FAIL: gate missing at $GATE"; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/dep/src" "$WORK/hkask-probe/src"
cat > "$WORK/Cargo.toml" <<'EOF'
[workspace]
members = ["dep", "hkask-probe"]
resolver = "2"
EOF

cat > "$WORK/dep/Cargo.toml" <<'EOF'
[package]
name = "dep_leaf"
version = "0.1.0"
edition = "2021"
EOF
printf 'pub fn val() -> u32 { 42 }\n' > "$WORK/dep/src/lib.rs"

cat > "$WORK/hkask-probe/Cargo.toml" <<'EOF'
[package]
name = "hkask-probe"
version = "0.1.0"
edition = "2021"

[dependencies]
dep_leaf = { path = "../dep" }
EOF
printf 'pub fn unused() -> u32 { 1 }\n' > "$WORK/hkask-probe/src/lib.rs"

# Generate Cargo.lock offline for the local path workspace.
cargo generate-lockfile --offline --manifest-path "$WORK/Cargo.toml" >/dev/null 2>&1

failures=0

# Case 1: unused dependency must exit 1 and name dep_leaf.
set +e
OUT1=$(KASK_UNUSED_DEPS_DIRS="$WORK/hkask-*" KASK_UNUSED_DEPS_MANIFEST_PATH="$WORK/Cargo.toml" bash "$GATE" 2>&1)
RC1=$?
set -e
if [ "$RC1" -ne 1 ] || ! printf '%s\n' "$OUT1" | grep -q "dep_leaf"; then
  echo "FAIL (case 1 — unused dep): expected exit 1 naming dep_leaf, got $RC1"
  printf '%s\n' "$OUT1" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 1 — unused dep): gate caught unused dep_leaf"
fi

# Case 2: used dependency must exit 0.
printf 'pub fn used() -> u32 { dep_leaf::val() }\n' > "$WORK/hkask-probe/src/lib.rs"
set +e
OUT2=$(KASK_UNUSED_DEPS_DIRS="$WORK/hkask-*" KASK_UNUSED_DEPS_MANIFEST_PATH="$WORK/Cargo.toml" bash "$GATE" 2>&1)
RC2=$?
set -e
if [ "$RC2" -ne 0 ]; then
  echo "FAIL (case 2 — used dep): expected exit 0, got $RC2"
  printf '%s\n' "$OUT2" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 2 — used dep): gate passed when dependency is referenced"
fi

# Case 3: empty scan dirs must fail closed (exit 1).
set +e
OUT3=$(KASK_UNUSED_DEPS_DIRS="$WORK/nonexistent-*" KASK_UNUSED_DEPS_MANIFEST_PATH="$WORK/Cargo.toml" bash "$GATE" 2>&1)
RC3=$?
set -e
if [ "$RC3" -ne 1 ]; then
  echo "FAIL (case 3 — empty scan dirs): expected exit 1, got $RC3"
  printf '%s\n' "$OUT3" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 3 — empty scan dirs): vacuous zero-crate scan rejected"
fi

# Case 4: compile error must fail closed (exit 1), never pass as 0 unused deps.
printf 'pub fn broken() -> u32 { SYNTAX_ERROR }\n' > "$WORK/hkask-probe/src/lib.rs"
set +e
OUT4=$(KASK_UNUSED_DEPS_DIRS="$WORK/hkask-*" KASK_UNUSED_DEPS_MANIFEST_PATH="$WORK/Cargo.toml" bash "$GATE" 2>&1)
RC4=$?
set -e
if [ "$RC4" -ne 1 ]; then
  echo "FAIL (case 4 — compile error): expected exit 1, got $RC4"
  printf '%s\n' "$OUT4" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 4 — compile error): broken build failed closed"
fi

if [ "$failures" -eq 0 ]; then
  echo "SELFTEST OK: check-unused-deps catches unused deps, empty scans, and compile errors"
  exit 0
fi
echo "SELFTEST FAIL: $failures case(s) failed" >&2
exit 1
