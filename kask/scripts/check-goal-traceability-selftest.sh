#!/usr/bin/env bash
# expect: [P8] skill goal traceability warns without blocking or trusting UUIDs.
set -euo pipefail
CHECKER="$(cd "$(dirname "$0")" && pwd)/check-goal-traceability.sh"
FIXTURE=$(mktemp -d)
trap 'rm -rf "$FIXTURE"' EXIT

# Name the failing case: a bare [[ ]] under set -e aborts silently, and a
# silent selftest failure shows CI only "exit code 1" (observed: the
# advisory job failed across every run for a day with zero diagnostic
# output, because one superseded expectation died without a message).
expect_output() { # <case-label> <needle>
    if [[ "$output" == *"$2"* ]]; then return 0; fi
    echo "FAIL ($1): expected output mentioning: $2" >&2
    echo "  got: $output" >&2
    exit 1
}

git -C "$FIXTURE" init -q
git -C "$FIXTURE" config user.name 'Traceability Fixture'
git -C "$FIXTURE" config user.email 'fixture@example.invalid'
git -C "$FIXTURE" config commit.gpgsign false
git -C "$FIXTURE" config core.hooksPath /dev/null
cd "$FIXTURE"
printf 'baseline\n' > README
git add README
git commit -qm baseline
BASE=$(git rev-parse HEAD)
mkdir -p .agents/skills/example
printf 'skill\n' > .agents/skills/example/SKILL.md
git add .agents
printf 'Change a skill\n' > message
output=$(bash "$CHECKER" --message message 2>&1)
expect_output 'missing goal' 'ADVISORY: missing or malformed Goal:'
printf '\nGoal: not-a-uuid\n' >> message
output=$(bash "$CHECKER" --message message 2>&1)
expect_output 'malformed goal' 'ADVISORY: missing or malformed Goal:'
# The Goal line matches anywhere in the message (the checker's documented
# ruling, 2574bd2216: trailer-position parsing false-fired on every
# convention-placed Goal line), so a Goal line followed by body text still
# counts — the strict-trailer expectation this case originally pinned was
# superseded by that fix and never updated (the silent-abort mode above is
# what let the drift hide).
printf 'Change a skill\n\nGoal: 11111111-1111-4111-8111-111111111111\n\nThis is body text, not a trailer.\n' > message
output=$(bash "$CHECKER" --message message 2>&1)
expect_output 'goal line with trailing body text' 'syntax only; goal existence and approval not verified'
printf 'Change a skill\n\nGoal: 11111111-1111-4111-8111-111111111111\n' > message
output=$(bash "$CHECKER" --message message 2>&1)
expect_output 'well-formed goal trailer' 'syntax only; goal existence and approval not verified'
printf 'Change a skill\n\nGoal-Exception: documentation-only; operator review pending\n' > message
output=$(bash "$CHECKER" --message message 2>&1)
expect_output 'exception claim' 'exception claim requires operator review'
git commit -qm 'Skill without goal (hook bypass fixture)'
git mv .agents/skills/example/SKILL.md outside.md
git commit -qm 'Move skill outside scope'
git mv outside.md .agents/skills/example/SKILL.md
git commit -qm 'Return skill'
git rm -q .agents/skills/example/SKILL.md
git commit -qm 'Delete skill'
output=$(bash "$CHECKER" --range "$BASE" HEAD 2>&1)
count=$(grep -c 'ADVISORY: missing or malformed Goal:' <<< "$output")
if [ "$count" -ne 4 ]; then
    echo "FAIL (range advisories): expected 4, got $count" >&2
    echo "  got: $output" >&2
    exit 1
fi
BEFORE_SQUASH=$(git rev-parse HEAD)
mkdir -p .agents/skills/second .agents/skills/example
printf 'first\n' > .agents/skills/example/SKILL.md
printf 'second\n' > .agents/skills/second/SKILL.md
git add .agents
printf 'Squash-style skill changes\n\nGoal: 11111111-1111-4111-8111-111111111111\n' > message
git commit -qF message
output=$(bash "$CHECKER" --range "$BEFORE_SQUASH" HEAD 2>&1)
expect_output 'squash-style goal' 'syntax only; goal existence and approval not verified'
# The real hook must remain nonblocking and retain its existing rejection rules.
mkdir -p kask/scripts kask/githooks
cp "$CHECKER" kask/scripts/
cp "$(dirname "$CHECKER")/../githooks/commit-msg" kask/githooks/
printf 'skill changed\n' >> .agents/skills/second/SKILL.md
git add .agents
printf 'Missing goal\n' > message
output=$(bash kask/githooks/commit-msg message 2>&1)
expect_output 'commit-msg hook advisory' 'ADVISORY: missing or malformed Goal:'
git commit -qm 'Fixture completes pending skill edit'
printf 'ordinary edit\n' >> README
git add README
output=$(bash "$CHECKER" --message message 2>&1)
expect_output 'no skill paths' 'no skill paths changed'
output=$(bash "$CHECKER" --range invalid-ref HEAD 2>&1)
expect_output 'invalid range' 'ADVISORY: traceability could not be evaluated'
echo 'goal-traceability selftest: all advisory cases passed'
