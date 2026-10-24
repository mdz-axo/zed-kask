#!/usr/bin/env bash
# Self-test for the pre-push hook (kask/githooks/pre-push): the net-lines
# reporting and the diagram-registry parity gate.
#
# The reporting job's RED proof is that the reported numbers are the TRUE
# numbers for a known range; the gate's RED proof (added 2026-10-23, eureka
# cycle EUREKA-1) is that a red parity tree BLOCKS the push and a green or
# restored tree does not:
#   1. A range with known content reports the true net line count and the
#      true DIVERGENCE.md seam delta (+1 added / -1 retired → net 0), and
#      exits 0 (with the parity gate green).
#   2. A new-branch push (zero remote sha) with no upstream remote
#      degrades VISIBLY (names the skip) instead of guessing a range.
#   3. A ref deletion (zero local sha) measures nothing.
#   4. A malformed range still exits 0 and reports "no diff in range".
#   5. A red parity tree (an unregistered mermaid fence) BLOCKS: exit 1
#      with the parity rejection named.
#   6. Balance restored → the hook allows again (exit 0).
#
# Runs in CI via the kask/scripts/check-*-selftest.sh glob.
# Exit codes: 0 = every case behaved as expected, 1 = the hook is wrong.

set -uo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOOK_SRC="$KASK_ROOT/githooks/pre-push"

[ -f "$HOOK_SRC" ] || { echo "FAIL: hook not found at $HOOK_SRC"; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
FAILURES=0

git init -q "$WORK"
git -C "$WORK" config user.email selftest@local
git -C "$WORK" config user.name selftest
mkdir -p "$WORK/githooks"
cp "$HOOK_SRC" "$WORK/githooks/pre-push"

# Base: three seam rows, five code lines.
printf '| D1 | base seam |\n| D2 | base seam |\n| D3 | base seam |\n' > "$WORK/DIVERGENCE.md"
printf 'a\nb\nc\nd\ne\n' > "$WORK/code.txt"
git -C "$WORK" add -A
git -C "$WORK" commit -qm base
BASE="$(git -C "$WORK" rev-parse HEAD)"

# Tip: one seam row added (D4), one retired (D2), two code lines removed.
# True diff: 1 insertion, 3 deletions → net -2; seams +1/-1 → net 0.
printf '| D1 | base seam |\n| D3 | base seam |\n| D4 | new seam |\n' > "$WORK/DIVERGENCE.md"
printf 'a\nb\nc\n' > "$WORK/code.txt"
git -C "$WORK" add -A
git -C "$WORK" commit -qm tip
TIP="$(git -C "$WORK" rev-parse HEAD)"

# The parity gate's fixture: the real gate script against a balanced
# synthetic docs tree (the parity selftest's own pattern), so the hook's
# gate arm is exercised end-to-end here.
mkdir -p "$WORK/kask/scripts" "$WORK/kask/docs"
cp "$KASK_ROOT/scripts/check-diagram-registry-parity.sh" "$WORK/kask/scripts/"
cat > "$WORK/kask/docs/registered.md" <<'EOF'
```mermaid
graph TD
    A --> B
```

<!-- DIAGRAM_ALIGNMENT
id: DIAG-TEST-001
verified_date: 2026-10-23
verified_against: selftest
status: VERIFIED
-->
EOF
cat > "$WORK/kask/docs/DIAGRAMS_INDEX.md" <<'EOF'
# Index

| DIAGRAM_ID | Location | Verified date | Status |
| --- | --- | --- | --- |
| `DIAG-TEST-001` | registered.md | 2026-10-23 | VERIFIED |
EOF

ZERO=0000000000000000000000000000000000000000

run_hook() { # run_hook <stdin-line...> — mirrors git's invocation (cwd = repo root)
  (cd "$WORK" && printf '%s\n' "$@" | bash githooks/pre-push 2>&1)
}

expect_contains() { # expect_contains <label> <needle> <haystack>
  if grep -qF -- "$2" <<<"$3"; then
    echo "ok: $1"
  else
    echo "FAIL: $1 — output did not contain: $2" >&2
    echo "--- got ---" >&2
    printf '%s\n' "$3" >&2
    echo "------------" >&2
    FAILURES=$((FAILURES + 1))
  fi
}

expect_exit() { # expect_exit <label> <expected> <actual>
  if [ "$3" -eq "$2" ]; then
    echo "ok: $1"
  else
    echo "FAIL: $1 — expected exit $2, got $3" >&2
    FAILURES=$((FAILURES + 1))
  fi
}

# Case 1: known range → true numbers, exit 0.
out="$(run_hook "refs/heads/main $TIP refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook exits 0 on a normal range" 0 "$rc"
expect_contains "known range reports true net lines" "net -2 lines" "$out"
expect_contains "known range reports true seam delta" "seams: +1 added / -1 retired" "$out"
expect_contains "known range reports zero seam net" "— net 0" "$out"

# Case 2: new branch, no upstream remote → visible skip.
out="$(run_hook "refs/heads/topic $TIP refs/heads/topic $ZERO")"
expect_contains "new branch without upstream names the skip" \
  "no upstream/main merge base — net-lines skipped" "$out"

# Case 3: ref deletion → no measurement at all.
out="$(run_hook "refs/heads/gone $ZERO refs/heads/gone $BASE")"
if [ -z "$out" ]; then
  echo "ok: ref deletion measures nothing"
else
  echo "FAIL: ref deletion should measure nothing, got: $out" >&2
  FAILURES=$((FAILURES + 1))
fi

# Case 4: malformed range → still exits 0, reports no diff.
out="$(run_hook "refs/heads/main $TIP refs/remotes/origin/main deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")"; rc=$?
expect_exit "hook exits 0 on a malformed range" 0 "$rc"
expect_contains "malformed range reports no diff" "no diff in range" "$out"

# Case 5: a red parity tree (unregistered mermaid fence) BLOCKS the push.
printf '```mermaid\ngraph TD\n    B --> C\n```\n' > "$WORK/kask/docs/unregistered.md"
out="$(run_hook "refs/heads/main $TIP refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook blocks a red parity tree" 1 "$rc"
expect_contains "red rejection names the parity gate" "diagram registry parity is red" "$out"
rm "$WORK/kask/docs/unregistered.md"

# Case 6: balance restored → the hook allows again.
out="$(run_hook "refs/heads/main $TIP refs/remotes/origin/main $BASE")"; rc=$?
expect_exit "hook allows once parity is restored" 0 "$rc"

if [ "$FAILURES" -eq 0 ]; then
  echo ""
  echo "SELFTEST OK: pre-push hook reports true numbers, and the parity gate blocks red and allows green (report RED + skip + deletion + malformed + gate red/green all pinned)"
  exit 0
fi
echo "" >&2
echo "SELFTEST FAIL: $FAILURES case(s) did not behave as expected." >&2
exit 1
