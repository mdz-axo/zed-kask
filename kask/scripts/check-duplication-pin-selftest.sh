#!/usr/bin/env bash
# Self-test for the duplication scan-base pin gate
# (check-duplication-pin.sh).
#
# Institutionalizes the "a CI gate must be shown to fail before its status:
# enforced is trusted" discipline for the pin-freshness gate. Pins the
# gate's behavior on a synthetic repo whose merge topology is exactly
# known:
#   1. The pin naming the merge's second parent PASSES.
#   2. A stale pin (an older commit) FAILS — the RED proof.
#   3. The pin naming the merge's first parent FAILS.
#   4. A missing pin file FAILS.
#
# Runs in CI via the kask/scripts/check-*-selftest.sh glob.
# Exit codes: 0 = all cases behaved as expected, 1 = the gate is vacuous
# or broken.

set -uo pipefail

# Shared synthetic-repo setup and pass/fail accounting (one implementation
# for every check-*-selftest.sh gate).
# shellcheck disable=SC1091  # the harness path is CWD-independent by design
. "$(cd "$(dirname "$0")" && pwd)/selftest-harness.sh"

selftest_init check-duplication-pin.sh

# A synthetic repo whose merge topology is exactly known. The initial
# branch is pinned with `git init -b main` — a bare `git init` names it
# from init.defaultBranch ("main" on the dev machine, "master" on the CI
# runner), and the `git checkout -q main` below then failed on the runner
# ("pathspec 'main' did not match", observed run 43536862391) — the merge
# never formed and case 1 read a one-parent commit as the merge tip.
git init -q -b main "$WORK"
git -C "$WORK" config user.email selftest@local
git -C "$WORK" config user.name selftest
mkdir -p "$WORK/scripts"
cp "$GATE" "$WORK/scripts/check-duplication-pin.sh"

# Base commit on main.
printf 'base\n' > "$WORK/base.txt"
git -C "$WORK" add -A
git -C "$WORK" commit -qm base
BASE="$(git -C "$WORK" rev-parse HEAD)"

# An upstream commit on a side branch, then merge it into main: the merge
# commit's second parent is the upstream tip.
git -C "$WORK" checkout -qb upstream HEAD~0
printf 'upstream\n' > "$WORK/upstream.txt"
git -C "$WORK" add -A
git -C "$WORK" commit -qm upstream
git -C "$WORK" checkout -q main
printf 'fork\n' > "$WORK/fork.txt"
git -C "$WORK" add -A
git -C "$WORK" commit -qm fork
MERGE="$(git -C "$WORK" merge --no-ff -q upstream -m merge 2>/dev/null; git -C "$WORK" rev-parse HEAD)"
SECOND="$(git -C "$WORK" rev-parse "${MERGE}^2")"
FIRST="$(git -C "$WORK" rev-parse "${MERGE}^1")"

# Case 1: the pin names the merge's second parent — passes.
printf '%s\n' "$SECOND" > "$WORK/scripts/duplication-scan-base.txt"
bash "$WORK/scripts/check-duplication-pin.sh" >/dev/null 2>&1
expect $? 0 "pin naming the merge's second parent passes"

# Case 2: a stale pin — fails (RED proof).
printf '%s\n' "$BASE" > "$WORK/scripts/duplication-scan-base.txt"
bash "$WORK/scripts/check-duplication-pin.sh" >/dev/null 2>&1
expect $? 1 "a stale pin fails"

# Case 3: the pin naming the merge's first parent — fails.
printf '%s\n' "$FIRST" > "$WORK/scripts/duplication-scan-base.txt"
bash "$WORK/scripts/check-duplication-pin.sh" >/dev/null 2>&1
expect $? 1 "a pin naming the first parent fails"

# Case 4: a missing pin file — fails.
rm "$WORK/scripts/duplication-scan-base.txt"
bash "$WORK/scripts/check-duplication-pin.sh" >/dev/null 2>&1
expect $? 1 "a missing pin file fails"

selftest_finish "duplication scan-base pin gate is alive (current passes; stale/first-parent/missing fail)"