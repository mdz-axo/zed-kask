#!/usr/bin/env bash
# Self-test for the code→docs citation gate (check-code-docs-citations.sh).
#
# Institutionalizes the .rules trap "a CI gate must be shown to fail
# before its status: enforced is trusted" (the check-docs-citations
# selftest pattern): every verdict class is pinned over fixture trees
# via CODE_CITES_ROOT:
#   1. Clean: a live file cites an existing doc → counted resolved, exit 0.
#   2. Rotted: a live file cites a deleted doc → ::error::, exit 1.
#   3. Historical: DIVERGENCE.md cites a deleted doc → warning, exit 0.
#   4. Fixture allowlist: the deliberate selftest script path cites a
#      deleted doc → ignored, exit 0.
#   5. Docs-internal: a kask/docs file cites a deleted doc → skipped
#      (the sibling gate's scope), exit 0.
#
# Exit codes: 0 = every case behaved as expected, 1 = a case misbehaved.

set -uo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-code-docs-citations.sh"
[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

FAILED=0
case_run() { # case_run <name> <expected-exit>
    local name=$1 want=$2 got
    CODE_CITES_ROOT="$CASE" bash "$GATE" >/dev/null 2>"$CASE/out"
    got=$?
    if [ "$got" -eq "$want" ]; then
        echo "  ok: $name (exit $got)"
    else
        echo "  FAIL: $name — expected exit $want, got $got" >&2
        cat "$CASE/out" >&2
        FAILED=1
    fi
}

# Case 1+2: a live file citing an existing doc alongside a rotted cite.
CASE="$WORK/rotten"; mkdir -p "$CASE/tools" "$CASE/kask/docs/reference"
echo "see kask/docs/reference/exists.md here" > "$CASE/tools/live.rs"
echo "see kask/docs/reference/gone.md here" > "$CASE/tools/rotten.rs"
echo "# exists" > "$CASE/kask/docs/reference/exists.md"
case_run "rotted live-surface cite fails" 1
grep -q '::error::code-docs-citation: tools/rotten.rs cites deleted kask/docs/reference/gone.md' "$CASE/out" \
    && echo "  ok: error line names the file and the deleted doc" \
    || { echo "  FAIL: error line missing or malformed" >&2; cat "$CASE/out" >&2; FAILED=1; }
rm "$CASE/tools/rotten.rs"
case_run "clean tree after excision passes" 0

# Case 3: historical-record scope warns, never fails.
CASE="$WORK/historical"; mkdir -p "$CASE" "$CASE/kask/docs/reference"
echo "seam provenance: kask/docs/reference/gone.md drove this row" > "$CASE/DIVERGENCE.md"
case_run "DIVERGENCE.md historical cite is a warning" 0
grep -q 'warn: DIVERGENCE.md cites deleted kask/docs/reference/gone.md (historical-record scope' "$CASE/out" \
    && echo "  ok: warning line emitted" \
    || { echo "  FAIL: historical warning missing" >&2; cat "$CASE/out" >&2; FAILED=1; }

# Case 4: the deliberate fixture is allowlisted.
CASE="$WORK/fixture"; mkdir -p "$CASE/kask/scripts"
echo "deliberate negative fixture: kask/docs/reference/this-doc-does-not-exist.md" > "$CASE/kask/scripts/check-skill-review-instruments-selftest.sh"
case_run "allowlisted fixture ignored" 0

# Case 5: docs-internal citations are the sibling gate's scope.
CASE="$WORK/internal"; mkdir -p "$CASE/kask/docs/plans" "$CASE/kask/docs/research"
echo "cross-ref kask/docs/research/gone.md" > "$CASE/kask/docs/plans/a-plan.md"
case_run "docs-internal cite skipped (sibling gate scope)" 0

echo "----"
if [ "$FAILED" -eq 0 ]; then
    echo "selftest PASS: every verdict class pinned"
    exit 0
fi
echo "selftest FAIL: a case misbehaved" >&2
exit 1
