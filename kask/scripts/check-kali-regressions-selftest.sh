#!/usr/bin/env bash
# Self-test for the kali-regressions gate.
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before its
# status: enforced is trusted" (added pass 2, made permanent pass 3). The gate
# reported green for weeks while three independent vacuity classes (single-quote
# pattern corruption, dead include paths, inverted presence semantics) made 20+
# entries unenforceable. This script injects synthetic violations of each class
# and asserts the gate actually fails — so a future regression in
# lib-regressions.sh cannot silently re-vacate the gate.
#
# Design: the real `security/regressions/` directory is COPIED to a temp dir,
# synthetic RR-TEST-*.yaml entries are added to the copy, and `check_regressions`
# is invoked with REGRESSIONS_DIR pointed at the copy. The real directory is
# never touched, and a parallel CI run of the real gate cannot pick up the
# synthetic entries (the trap called out in the pass-3 task brief).
#
# Exit codes:
#   0 — all three synthetic violations were detected (gate is alive)
#   1 — at least one synthetic violation was NOT detected (gate is vacuous)
#
# Usage: bash scripts/check-kali-regressions-selftest.sh

set -euo pipefail

# Locate the kask/ root from this script's location (scripts/ lives under kask/).
KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REAL_REGRESSIONS_DIR="$KASK_ROOT/security/regressions"
LIB="$KASK_ROOT/scripts/lib-regressions.sh"

if [ ! -d "$REAL_REGRESSIONS_DIR" ]; then
  echo "FAIL: real regressions dir not found at $REAL_REGRESSIONS_DIR"
  exit 1
fi
if [ ! -f "$LIB" ]; then
  echo "FAIL: lib-regressions.sh not found at $LIB"
  exit 1
fi

# shellcheck source=scripts/lib-regressions.sh
source "$LIB"

# Temp workspace. trap cleans up on every exit path.
TMPDIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMPDIR"
}
trap cleanup EXIT

# Copy the real regressions dir so the synthetic entries inherit a realistic
# sibling set (and so the orphan-detector's "at least one include exists" check
# has real files to compare against). The copy is under TMPDIR, never under
# security/regressions/, so a parallel real-gate CI run cannot pick it up.
cp -r "$REAL_REGRESSIONS_DIR" "$TMPDIR/regressions"
# Plant a temp file the presence-violation entry will look for (and not find).
PLANTED_FILE="$TMPDIR/regressions/SELFTEST_PLANTED_TARGET.rs"
cat > "$PLANTED_FILE" <<'EOF'
// selftest planted target — must not leak into the real regressions dir
fn real_fn() {}
EOF

# A temp source file the absence-violation entry will scan and find the banned
# pattern in. Lives under TMPDIR so the real gate never sees it.
ABSENCE_SRC="$TMPDIR/regressions/SELFTEST_ABSENCE_SRC.rs"
cat > "$ABSENCE_SRC" <<'EOF'
// selftest: this file deliberately contains the banned pattern
fn bad() { McpToolError::internal(format!("selftest banned pattern")); }
EOF

failures=0

# Helper: run the gate against the temp regressions copy from the kask root.
# Running from kask/ (not TMPDIR) keeps the real entries' relative include
# paths resolving against the real source tree — only REGRESSIONS_DIR is
# redirected to the temp copy. Synthetic entries use absolute include paths
# so they resolve regardless of cwd.
run_gate() {
  (cd "$KASK_ROOT" && KASK_REGRESSIONS_DIR="$TMPDIR/regressions" \
    check_regressions "" "" "reg-span" 2>&1)
}

# ──────────────────────────────────────────────────────────────────────────
# Case 1: orphaned include path → gate must fail with "orphaned"
# ──────────────────────────────────────────────────────────────────────────
cat > "$TMPDIR/regressions/RR-TEST-ORPHAN.yaml" <<'EOF'
id: RR-TEST-ORPHAN
title: "selftest: orphaned include path must be detected"
surface: llm-io
detection:
  kind: grep
  pattern: 'McpToolError::internal'
  include: "this/path/does/not/exist/anywhere.rs"
status: enforced
EOF

set +e
ORPHAN_OUT="$(run_gate)"
ORPHAN_RC=$?
set -e

if [ "$ORPHAN_RC" -ne 1 ]; then
  echo "FAIL (case 1 — orphaned path): expected exit 1, got $ORPHAN_RC"
  echo "  output:"
  printf '%s\n' "$ORPHAN_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$ORPHAN_OUT" | grep -q "orphaned"; then
  echo "FAIL (case 1 — orphaned path): exit 1 but 'orphaned' not in output"
  printf '%s\n' "$ORPHAN_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 1 — orphaned path): gate detected the dead include path"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 2: presence semantics with an unmatchable pattern → gate must fail
# with "presence"
# ──────────────────────────────────────────────────────────────────────────
cat > "$TMPDIR/regressions/RR-TEST-PRESENCE.yaml" <<EOF
id: RR-TEST-PRESENCE
title: "selftest: presence invariant must fire when pattern is absent"
surface: llm-io
detection:
  kind: grep
  semantics: presence
  pattern: 'ThisExactStringDoesNotExistAnywhereInKaskSource__selftest_xyzzy'
  include: "$PLANTED_FILE"
status: enforced
EOF

set +e
PRESENCE_OUT="$(run_gate)"
PRESENCE_RC=$?
set -e

if [ "$PRESENCE_RC" -ne 1 ]; then
  echo "FAIL (case 2 — presence): expected exit 1, got $PRESENCE_RC"
  printf '%s\n' "$PRESENCE_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$PRESENCE_OUT" | grep -q "presence"; then
  echo "FAIL (case 2 — presence): exit 1 but 'presence' not in output"
  printf '%s\n' "$PRESENCE_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 2 — presence): gate detected the missing presence pattern"
fi

# ──────────────────────────────────────────────────────────────────────────
# Case 3: absence semantics with a pattern that IS present in a planted file
# → gate must fail with "violated"
# ──────────────────────────────────────────────────────────────────────────
cat > "$TMPDIR/regressions/RR-TEST-ABSENCE.yaml" <<EOF
id: RR-TEST-ABSENCE
title: "selftest: absence invariant must fire when banned pattern is present"
surface: llm-io
detection:
  kind: grep
  pattern: 'McpToolError::internal\(format!'
  include: "$ABSENCE_SRC"
status: enforced
EOF

set +e
ABSENCE_OUT="$(run_gate)"
ABSENCE_RC=$?
set -e

if [ "$ABSENCE_RC" -ne 1 ]; then
  echo "FAIL (case 3 — absence): expected exit 1, got $ABSENCE_RC"
  printf '%s\n' "$ABSENCE_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
elif ! printf '%s\n' "$ABSENCE_OUT" | grep -q "violated"; then
  echo "FAIL (case 3 — absence): exit 1 but 'violated' not in output"
  printf '%s\n' "$ABSENCE_OUT" | sed 's/^/    /'
  failures=$((failures + 1))
else
  echo "OK (case 3 — absence): gate detected the present banned pattern"
fi

# ──────────────────────────────────────────────────────────────────────────
# Verdict
# ──────────────────────────────────────────────────────────────────────────
if [ "$failures" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: kali-regressions gate is alive (all 3 synthetic violations detected)"
  exit 0
else
  echo ""
  echo "SELFTEST FAIL: $failures case(s) did not detect their synthetic violation"
  echo "The gate is vacuous — see kask/scripts/lib-regressions.sh."
  exit 1
fi
