#!/usr/bin/env bash
# Self-test for the dead-code-allow gate (check-no-new-dead-code-allows.sh).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before
# its status: enforced is trusted" — the same pattern the (since-removed)
# skill-span-namespace selftest documented. Pins the gate's behavior:
#   1. A tree at its baseline passes.
#   2. A net-new allow in a baselined file FAILS (the RED proof).
#   3. An allow in a file absent from the baseline FAILS.
#   4. Removals (below baseline) pass — the gate never punishes cleanup.
#   5. --refresh re-baselines the tree and it passes again.
#
# Run locally: bash kask/scripts/check-no-new-dead-code-allows-selftest.sh
# Exit codes: 0 = all cases behaved as expected, 1 = the gate is vacuous or
# broken.
set -uo pipefail

GATE="$(cd "$(dirname "$0")" && pwd)/check-no-new-dead-code-allows.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAILURES=0

expect() { # expect <actual-exit> <expected-exit> <label>
  if [ "$1" -ne "$2" ]; then
    echo "FAIL: $3 (expected exit $2, got $1)"
    FAILURES=$((FAILURES + 1))
  else
    echo "ok: $3"
  fi
}

# The gate reads the git index (git ls-files), so stage before each run.
gate_exit() {
  git -C "$WORK" add -A
  bash "$WORK/scripts/check-no-new-dead-code-allows.sh" >/dev/null 2>&1
  return $?
}

# Build a synthetic repo the gate can inspect.
mkdir -p "$WORK/scripts" "$WORK/src"
cp "$GATE" "$WORK/scripts/check-no-new-dead-code-allows.sh"
git -C "$WORK" init -q
git -C "$WORK" config user.email selftest@local
git -C "$WORK" config user.name selftest

cat > "$WORK/src/lib.rs" <<'EOF'
#[allow(dead_code)] // deserialization-only field
struct A {
    x: u32,
}
#[allow(dead_code)]
struct B {
    x: u32,
}
EOF
git -C "$WORK" add -A
git -C "$WORK" commit -qm baseline

# Seed the baseline at the current (legitimate) count: lib.rs:2.
bash "$WORK/scripts/check-no-new-dead-code-allows.sh" --refresh >/dev/null

# Case 1: at baseline → pass.
gate_exit
expect $? 0 "at-baseline tree passes"

# Case 2: +1 allow in a baselined file → FAIL (the RED proof).
cat >> "$WORK/src/lib.rs" <<'EOF'
#[allow(dead_code)]
struct C {
    x: u32,
}
EOF
gate_exit
expect $? 1 "a net-new allow in a baselined file fails"

# Case 3: an allow in a new file, absent from the baseline → FAIL.
cat > "$WORK/src/extra.rs" <<'EOF'
#[allow(dead_code)]
struct D {
    x: u32,
}
EOF
gate_exit
expect $? 1 "an allow in a file absent from the baseline fails"

# Case 4: removal (below baseline) → pass — cleanup is never punished.
cat > "$WORK/src/lib.rs" <<'EOF'
#[allow(dead_code)] // deserialization-only field
struct A {
    x: u32,
}
EOF
rm "$WORK/src/extra.rs"
gate_exit
expect $? 0 "removals below baseline pass"

# Case 5: --refresh re-baselines the tree → pass.
bash "$WORK/scripts/check-no-new-dead-code-allows.sh" --refresh >/dev/null
gate_exit
expect $? 0 "refresh re-baselines the tree"

if [ "$FAILURES" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: dead-code-allow gate is alive (RED + new-file + removal + refresh all pinned)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $FAILURES case(s) did not behave as expected — the gate is vacuous or broken."
  exit 1
fi
