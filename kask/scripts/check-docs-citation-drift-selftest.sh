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
#   7. Dangling baseline (unreachable audit hash) → the tier skips with a
#      surfaced SKIPPED status in the summary — never a masked clean 0
#      (the live defect: a baseline naming a commit lost to history
#      collapse false-cleaned the tier for days).
#   8. Export mode (no .git) → the same surfaced-skip contract in the
#      summary, which the pre-push hook prints on every push.
#   9. Continuation inheritance across lines: a continuation on the line
#      directly after a cited line inherits its path (and is checked —
#      a wrong symbol there exits 1); a citation-free line in between
#      ends inheritance, and the continuation is an orphan, not a pass
#      against a stale path.
#  10. A citation on an unterminated final line (a doc with no trailing
#      newline) is checked — a per-line `read` loop drops that line, and
#      21 of the 102 real docs end without a newline (2026-11-11).
#
# Exit codes: 0 = every case behaved as expected, 1 = a case misbehaved.
set -uo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-docs-citation-drift.sh"
[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

WORK="$(mktemp -d)"
# NOGIT lives OUTSIDE the fixture repo: the export-mode case must not see
# a parent .git through directory discovery (a nogit under $WORK found
# the fixture repo and took the wrong branch — the live specimen for the
# "validate the instrument against the real command form" rule).
NOGIT="$(mktemp -d)"
trap 'rm -rf "$WORK" "$NOGIT"' EXIT
git init -q "$WORK"
git -C "$WORK" config user.email selftest@local
git -C "$WORK" config user.name selftest

# Run the gate from a fixture-local copy: $0's SCRIPT_DIR decides the
# baseline path, and a --refresh case writing into the REAL
# kask/scripts/citation-drift-audit-hash.txt is selftest pollution of
# the ratchet it pins.
mkdir -p "$WORK/kask/scripts" "$WORK/kask/ast-grep/rules"
cp "$GATE" "$WORK/kask/scripts/"
cp "$KASK_ROOT/scripts/ast-grep.sh" "$WORK/kask/scripts/"
cp "$KASK_ROOT/ast-grep/rules/rust-definitions.yml" "$WORK/kask/ast-grep/rules/"
cp "$KASK_ROOT/ast-grep/rules/rust-impls.yml" "$WORK/kask/ast-grep/rules/"
GATE="$WORK/kask/scripts/check-docs-citation-drift.sh"

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

# Case 7: dangling baseline (unreachable audit hash) → surfaced SKIPPED in
# the summary, never a masked clean 0. The bogus hash must not exist in the
# fixture repo, so the reachability check fails the way a history-collapse
# orphan does in the real repo.
echo 0000000000000000000000000000000000000009 > "$WORK/kask/scripts/citation-drift-audit-hash.txt"
doc_with dangling '`src/widget.rs:4`'
out=$(run_gate dangling); rc=$?
expect 0 "$rc" "dangling baseline warns, not fails"
echo "$out" | grep -q 'is not a reachable commit' \
  || { echo "FAIL: dangling case missing the unreachable-baseline warn"; FAILED=1; }
echo "$out" | grep -q 'drift-window tier SKIPPED (audit hash' \
  || { echo "FAIL: dangling case summary did not surface the skip"; FAILED=1; }
echo "$out" | grep -qE '[0-9]+ shifted-region warnings' \
  && { echo "FAIL: dangling case printed a shifted-region count over a dead tier"; FAILED=1; }

# Case 8: export mode (no .git) → the skip surfaces in the summary too, so
# a hook-side run can never read as a clean 0 either. The shifted fixture doc
# carries no @sym cite (its plain cite must not redden the @sym phase).
mkdir -p "$NOGIT/src" "$NOGIT/docs/shifted"
cp "$WORK/src/widget.rs" "$NOGIT/src/widget.rs"
cp "$WORK/docs/shifted/doc.md" "$NOGIT/docs/shifted/doc.md"
out=$( REPO_ROOT="$NOGIT" DOCS_ROOT="docs/shifted" bash "$GATE" 2>&1 ); rc=$?
expect 0 "$rc" "export mode exits 0"
echo "$out" | grep -q 'drift-window tier SKIPPED (export mode, no .git)' \
  || { echo "FAIL: export case summary did not surface the skip"; FAILED=1; }
echo "$out" | grep -qE '[0-9]+ shifted-region warnings' \
  && { echo "FAIL: export case printed a shifted-region count over a skipped tier"; FAILED=1; }

# Case 9: continuation inheritance across lines. In the shifted fixture
# alpha spans 3-5 and beta spans 7-9.
mkdir -p "$WORK/docs/adjacent" "$WORK/docs/adjacent-bad" "$WORK/docs/gap"
printf 'See `src/widget.rs:4@alpha`\n`:8@beta` on the next line\n' > "$WORK/docs/adjacent/doc.md"
out=$(run_gate adjacent); rc=$?
expect 0 "$rc" "continuation on the adjacent line inherits the path"
echo "$out" | grep -q '0 orphaned continuation forms' \
  || { echo "FAIL: adjacent-line continuation was orphaned: $out"; FAILED=1; }
printf 'See `src/widget.rs:4@alpha`\n`:4@beta` on the next line\n' > "$WORK/docs/adjacent-bad/doc.md"
out=$(run_gate adjacent-bad); rc=$?
expect 1 "$rc" "adjacent-line continuation with the wrong symbol"
printf 'See `src/widget.rs:4@alpha`\nprose with no citation\n`:8@beta` after the gap\n' > "$WORK/docs/gap/doc.md"
out=$(run_gate gap); rc=$?
expect 0 "$rc" "continuation after a citation-free line is an orphan, not a failure"
echo "$out" | grep -q '1 orphaned continuation forms' \
  || { echo "FAIL: gap case did not orphan the continuation: $out"; FAILED=1; }

# Case 10: a mismatched citation on an unterminated final line still fails.
mkdir -p "$WORK/docs/unterminated"
printf 'intro\n`src/widget.rs:4@beta`' > "$WORK/docs/unterminated/doc.md"
out=$(run_gate unterminated); rc=$?
expect 1 "$rc" "mismatch on an unterminated final line"

[ "$FAILED" -eq 0 ] && echo "PASS: all citation-drift self-test cases behaved as expected."
exit "$FAILED"