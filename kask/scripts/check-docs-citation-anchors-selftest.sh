#!/usr/bin/env bash
# Self-test for check-docs-citation-anchors.sh.
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before
# its status: enforced is trusted": a gate that cannot fail is not a gate.
# Pins the verdict classes over a temp git repo whose scoped file is a
# fixture, with DOCS_ROOT pointed at per-case docs dirs:
#   1. Correct annotation (`fixture.rs:LINE@symbol` where the symbol encloses
#      LINE) → exit 0, counted annotated.
#   2. Mismatched annotation (wrong symbol) → exit 1 with ::error::.
#   3. Annotation whose line drifted past its symbol (line beyond the
#      definition, inside no definition) → exit 1 with ::error::.
#   4. Unannotated citation into a scoped file → warning only, exit 0.
#   5. Range form `file.rs:LINE-END@symbol` annotates by the range's START
#      line → exit 0 when the symbol encloses the start.
#
# Exit codes: 0 = every case behaved as expected, 1 = a case misbehaved.
#
# Usage: bash kask/scripts/check-docs-citation-anchors-selftest.sh

set -uo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-docs-citation-anchors.sh"
RESOLVER="$KASK_ROOT/scripts/resolve-citation-anchor.sh"

[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }
[ -f "$RESOLVER" ] || { echo "FAIL: resolver not found at $RESOLVER"; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
git init -q "$WORK"
git -C "$WORK" config user.email selftest@local
git -C "$WORK" config user.name selftest

# A scoped fixture file: the gate's scoped set matches by basename, so name
# it one of the scoped basenames (settings.rs) with a definition whose line
# we can cite. `alpha` spans lines 1-3, `beta` spans 5-7.
mkdir -p "$WORK/src"
cat > "$WORK/src/settings.rs" <<'RS'
pub fn alpha() -> u32 {
    1
}

pub fn beta() -> u32 {
    2
}
RS
git -C "$WORK" add -A

FAILURES=0

run_gate() {
  # $1 = docs dir (absolute), returns the gate's exit code via global RC.
  # Run from INSIDE the fixture repo so `git rev-parse --show-toplevel`
  # resolves to the fixture, not the real tree (the existing citations
  # selftest's discipline).
  (cd "$WORK" && DOCS_ROOT="$1" RESOLVER="$RESOLVER" bash "$GATE") >"$WORK/out.txt" 2>&1
  RC=$?
}

expect_rc() {
  local want=$1 label=$2
  if [ "$RC" -ne "$want" ]; then
    echo "FAIL: $label — expected rc=$want, got rc=$RC"
    sed 's/^/    /' "$WORK/out.txt" | tail -5
    FAILURES=$((FAILURES + 1))
  else
    echo "ok: $label (rc=$RC)"
  fi
}

D="$WORK/docs"

# Case 1 — correct annotation (alpha encloses line 1).
mkdir -p "$D"; printf 'See `src/settings.rs:1@alpha`.\n' > "$D/c1.md"
run_gate "$D"; expect_rc 0 "correct annotation passes"

# Case 2 — mismatched annotation.
printf 'See `src/settings.rs:1@beta`.\n' > "$D/c1.md"
run_gate "$D"; expect_rc 1 "mismatched symbol fails"
grep -q '::error::citation-anchor' "$WORK/out.txt" || { echo "FAIL: case 2 emitted no ::error::"; FAILURES=$((FAILURES+1)); }

# Case 3 — drifted annotation (line 4 is blank, inside no definition).
printf 'See `src/settings.rs:4@alpha`.\n' > "$D/c1.md"
run_gate "$D"; expect_rc 1 "drifted line (no enclosing def) fails"

# Case 4 — unannotated citation into a scoped file (grandfathered).
printf 'See `src/settings.rs:1`.\n' > "$D/c1.md"
run_gate "$D"; expect_rc 0 "unannotated scoped citation warns only"

# Case 5 — range form annotates by the start line.
printf 'See `src/settings.rs:5-7@beta`.\n' > "$D/c1.md"
run_gate "$D"; expect_rc 0 "range annotation passes by start line"

rm -rf "$D" "$WORK"

if [ "$FAILURES" -ne 0 ]; then
  echo "SELFTEST FAILED: $FAILURES case(s) misbehaved"
  exit 1
fi
echo "SELFTEST OK: citation-anchor gate fails on mismatch and drift, passes correct and grandfathered forms"
exit 0
