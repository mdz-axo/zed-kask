#!/usr/bin/env bash
# expect: [P8] skill goal traceability warns without blocking or trusting UUIDs.
set -euo pipefail
CHECKER="$(cd "$(dirname "$0")" && pwd)/check-goal-traceability.sh"
FIXTURE=$(mktemp -d)
trap 'rm -rf "$FIXTURE"' EXIT
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
[[ "$output" == *'ADVISORY: missing or malformed Goal:'* ]]
printf '\nGoal: not-a-uuid\n' >> message
output=$(bash "$CHECKER" --message message 2>&1)
[[ "$output" == *'ADVISORY: missing or malformed Goal:'* ]]
printf 'Change a skill\n\nGoal: 11111111-1111-4111-8111-111111111111\n\nThis is body text, not a trailer.\n' > message
output=$(bash "$CHECKER" --message message 2>&1)
[[ "$output" == *'ADVISORY: missing or malformed Goal:'* ]]
printf 'Change a skill\n\nGoal: 11111111-1111-4111-8111-111111111111\n' > message
output=$(bash "$CHECKER" --message message 2>&1)
[[ "$output" == *'syntax only; goal existence and approval not verified'* ]]
printf 'Change a skill\n\nGoal-Exception: documentation-only; operator review pending\n' > message
output=$(bash "$CHECKER" --message message 2>&1)
[[ "$output" == *'exception claim requires operator review'* ]]
git commit -qm 'Skill without goal (hook bypass fixture)'
git mv .agents/skills/example/SKILL.md outside.md
git commit -qm 'Move skill outside scope'
git mv outside.md .agents/skills/example/SKILL.md
git commit -qm 'Return skill'
git rm -q .agents/skills/example/SKILL.md
git commit -qm 'Delete skill'
output=$(bash "$CHECKER" --range "$BASE" HEAD 2>&1)
[[ $(grep -c 'ADVISORY: missing or malformed Goal:' <<< "$output") -eq 4 ]]
BEFORE_SQUASH=$(git rev-parse HEAD)
mkdir -p .agents/skills/second .agents/skills/example
printf 'first\n' > .agents/skills/example/SKILL.md
printf 'second\n' > .agents/skills/second/SKILL.md
git add .agents
printf 'Squash-style skill changes\n\nGoal: 11111111-1111-4111-8111-111111111111\n' > message
git commit -qF message
output=$(bash "$CHECKER" --range "$BEFORE_SQUASH" HEAD 2>&1)
[[ "$output" == *'syntax only; goal existence and approval not verified'* ]]
# The real hook must remain nonblocking and retain its existing rejection rules.
mkdir -p kask/scripts kask/githooks
cp "$CHECKER" kask/scripts/
cp "$(dirname "$CHECKER")/../githooks/commit-msg" kask/githooks/
printf 'skill changed\n' >> .agents/skills/second/SKILL.md
git add .agents
printf 'Missing goal\n' > message
output=$(bash kask/githooks/commit-msg message 2>&1)
[[ "$output" == *'ADVISORY: missing or malformed Goal:'* ]]
git commit -qm 'Fixture completes pending skill edit'
printf 'ordinary edit\n' >> README
git add README
output=$(bash "$CHECKER" --message message 2>&1)
[[ "$output" == *'no skill paths changed'* ]]
output=$(bash "$CHECKER" --range invalid-ref HEAD 2>&1)
[[ "$output" == *'ADVISORY: traceability could not be evaluated'* ]]
echo 'goal-traceability selftest: all advisory cases passed'
