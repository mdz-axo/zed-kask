#!/usr/bin/env bash
# Self-test for the structural-traps gate (check-structural-traps.sh).
#
# Pins the gate's four behaviors:
#   1. Ratchet pass: every current instance baselined → exit 0, and the
#      decoy classes (#[cfg(test)] mod, string literal, comment) are NOT
#      counted — the count assertion pins the exclusions the rule exists
#      for (a textual grep counts all three).
#   2. Net-new: an instance missing from the baseline → exit 1 naming it.
#   3. Empty scan surface → exit 0 (the 0 = 0 path the unsafe-forbid
#      selftest pins).
#   4. Stale baseline entry (a removal) → exit 0 with a stale note —
#      removals never fail.
#
# Case 1 generates its baseline through the gate's --refresh, pinning the
# refresh path too.
#
# Exit codes: 0 = every case behaved as expected, 1 = a case misbehaved.
set -uo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-structural-traps.sh"

[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/crates/selftest-widget/src"

# Production instances: use_one (line 5), use_two (lines 7-8, the
# multi-line receiver form). Decoys the rule must NOT count: comment
# (line 1), string literal (line 9), #[cfg(test)] mod (lines 10-15).
cat > "$WORK/crates/selftest-widget/src/hkask_widget.rs" <<'EOF'
// comment mentioning .unwrap() here
fn prod_a() -> Option<u32> { Some(2) }
pub fn use_one() -> u32 {
    prod_a().unwrap()
}
pub fn use_two() -> u32 {
    prod_a()
        .unwrap()
}
pub fn decoy_literal() -> String { ".unwrap() in a literal".to_string() }
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t() { assert_eq!(prod_a().unwrap(), 2); }
}
EOF
cat > "$WORK/crates/selftest-widget/src/tests.rs" <<'EOF'
#[test]
fn standalone_test_file_decoy() {
    assert_eq!(Some(2).unwrap(), 2);
}
EOF

FAILURES=0

# ── Case 1: ratchet pass + decoy exclusion ────────────────────────────────
# Env assignments live INSIDE each command substitution — a bare
# `SCAN_DIRS=... out=$(...)` prefix is an assignment prefix to an
# assignment and persists in the current shell (observed live: case 3's
# empty glob leaked into case 4).
out=$(SCAN_DIRS="$WORK/crates/*/src" BASELINE_FILE="$WORK/baseline.txt" bash "$GATE" --refresh 2>&1); rc=$?
if [ $rc -eq 0 ] && grep -q 'baseline refreshed: 2 instance(s)' <<<"$out"; then
  echo "case 1a (--refresh records exactly the 2 production instances): OK"
else
  echo "case 1a: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi
out=$(SCAN_DIRS="$WORK/crates/*/src" BASELINE_FILE="$WORK/baseline.txt" bash "$GATE" 2>&1); rc=$?
if [ $rc -eq 0 ] && grep -q '2 current instance(s), 0 net-new' <<<"$out"; then
  echo "case 1b (baselined surface passes, decoys excluded): OK"
else
  echo "case 1b: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi

# ── Case 2: net-new instance fails ───────────────────────────────────────
grep ':5$' "$WORK/baseline.txt" > "$WORK/baseline-missing.txt"
out=$(SCAN_DIRS="$WORK/crates/*/src" BASELINE_FILE="$WORK/baseline-missing.txt" bash "$GATE" 2>&1); rc=$?
if [ $rc -eq 1 ] && grep -q 'net-new' <<<"$out" && grep -q ':7' <<<"$out"; then
  echo "case 2 (unbaselined instance exits 1 naming it): OK"
else
  echo "case 2: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi

# ── Case 3: empty scan surface ────────────────────────────────────────────
out=$(SCAN_DIRS="$WORK/nothing/*/src" BASELINE_FILE="$WORK/baseline.txt" bash "$GATE" 2>&1); rc=$?
if [ $rc -eq 0 ] && grep -q 'no scan dirs match' <<<"$out"; then
  echo "case 3 (empty surface exits 0, no crash): OK"
else
  echo "case 3: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi

# ── Case 4: stale baseline entry (a removal) ──────────────────────────────
cp "$WORK/baseline.txt" "$WORK/baseline-stale.txt"
echo "$WORK/crates/selftest-widget/src/hkask_widget.rs:99" >> "$WORK/baseline-stale.txt"
out=$(SCAN_DIRS="$WORK/crates/*/src" BASELINE_FILE="$WORK/baseline-stale.txt" bash "$GATE" 2>&1); rc=$?
if [ $rc -eq 0 ] && grep -q '1 stale' <<<"$out"; then
  echo "case 4 (stale entry notes, never fails): OK"
else
  echo "case 4: FAIL (rc=$rc)"; printf '%s\n' "$out"; FAILURES=$((FAILURES + 1))
fi

echo "----"
if [ "$FAILURES" -eq 0 ]; then
  echo "structural-traps selftest: all 5 cases behaved as expected"
else
  echo "structural-traps selftest: $FAILURES case(s) misbehaved"
fi
[ "$FAILURES" -eq 0 ]