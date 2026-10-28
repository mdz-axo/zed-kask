#!/usr/bin/env bash
# Self-test for check-docs-citation-drift.sh.
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before
# its status is enforced is trusted": a gate that cannot fail is not a gate.
# Pins the ratchet's verdict classes over a temp git repo with DOCS_ROOT
# pointed at per-case fixture docs:
#   1. Correct annotation in an UNSCOPED file (the anchors gate cannot see
#      it; this gate must) → contributes 0 mismatches.
#   2. Correct CONTINUATION form (`:LINE@sym` inheriting the path from the
#      same line) → 0 mismatches.
#   3. Mismatched annotation in an unscoped file → exit 1 with ::error::.
#   4. Annotation drifted past its symbol (no enclosing definition) →
#      exit 1 with ::error::.
#   5. Plain citation in a region SHIFTED since the audit hash → warning,
#      not failure (exit 0 when no @sym mismatch).
#   6. --refresh writes the current HEAD hash into the baseline.
#
# Exit codes: 0 = every case behaved as expected, 1 = a case misbehaved.
set -uo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-docs-citation-drift.sh"
[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
git init -q "$WORK"
git -C "$WORK" config user.email selftest@local
git -C "$WORK" config user.name selftest

# Fixture source: an UNSCOPED basename (the anchors gate's scoped set does
# not contain widget.rs). alpha spans 1-3, beta spans 5-7.
mkdir -p "$WORK/src" "$WORK/docs"
cat > "$WORK/src/widget.rs" <<'RS'
pub fn alpha() -> u32 {
    1
}

pub fn beta() -> u32 {
    2
}
RS

doc_with() { # $1 = file name, $2 = citation text
  mkdir -p "$WORK/docs/$1"
  printf 'See %s\n' "$2" > "$WORK/docs/$1/doc.md"
}

run_gate() { # $1 = docs dir name
  ( cd "$WORK" && DOCS_ROOT="docs/$1" bash "$GATE" 2>&1 )
}

FAILED=0
expect() { # $1 = expected exit, $2 = actual exit, $3 = case name
  if [ "$1" != "$2" ]; then
    echo "FAIL: case $3 expected exit $1, got $2"
    FAILED=1
  else
    echo "ok: case $3 (exit $2)"
  fi
}

git -C "$WORK" add -A >/dev/null
git -C "$WORK" commit -qm fixture >/dev/null

# Case 1+2: good full-path annotation in an unscoped file + good continuation.
doc_with good '`src/widget.rs:2@alpha`, `:6@beta`'
out=$(run_gate good); rc=$?
expect 0 "$rc" "good full-path + continuation"
echo "$out" | grep -q 'citation drift: 0 @sym mismatches' \
  || { echo "FAIL: good case reported mismatches: $out"; FAILED=1; }

# Case 3: mismatched annotation → exit 1.
doc_with mismatched '`src/widget.rs:2@beta`'
out=$(run_gate mismatched); rc=$?
expect 1 "$rc" "mismatched annotation"
echo "$out" | grep -q '::error::citation-drift:' \
  || { echo "FAIL: mismatch case emitted no ::error::"; FAILED=1; }

# Case 4: drifted past symbol (line 99 in no definition) → exit 1.
doc_with drifted '`src/widget.rs:99@alpha`'
out=$(run_gate drifted); rc=$?
expect 1 "$rc" "drifted past symbol"
echo "$out" | grep -q 'no enclosing definition' \
  || { echo "FAIL: drift case missing no-enclosing-definition error"; FAILED=1; }

# Case 5: plain citation in a region shifted since the audit hash → warn.
( cd "$WORK" && DOCS_ROOT="docs/good" bash "$GATE" --refresh >/dev/null 2>&1 )
cat > "$WORK/src/widget.rs" <<'RS'
// two inserted lines shift alpha down
//
pub fn alpha() -> u32 {
    1
}

pub fn beta() -> u32 {
    2
}
RS
git -C "$WORK" add -A >/dev/null
git -C "$WORK" commit -qm shift >/dev/null
doc_with shifted '`src/widget.rs:4`'
out=$(run_gate shifted); rc=$?
expect 0 "$rc" "shifted plain citation warns, not fails"
echo "$out" | grep -q 'shifted/edited since the audit hash' \
  || { echo "FAIL: shifted case missing drift warning"; FAILED=1; }
echo "$out" | grep -qE 'citation drift: 0 @sym mismatches \(failures\), [1-9]' \
  || { echo "FAIL: shifted case did not count the warning"; FAILED=1; }

# Case 6: --refresh pins the current HEAD into the baseline.
out=$( cd "$WORK" && DOCS_ROOT="docs/good" bash "$GATE" --refresh 2>&1 )
echo "$out" | grep -q 'audit hash refreshed' \
  || { echo "FAIL: --refresh did not report"; FAILED=1; }

[ "$FAILED" -eq 0 ] && echo "PASS: all citation-drift self-test cases behaved as expected."
exit "$FAILED"