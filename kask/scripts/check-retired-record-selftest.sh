#!/usr/bin/env bash
# Self-test for the retired-record completeness gate
# (check-retired-record.sh): a retirement missing from RETIRED_SKILL_NAMES
# fails; a recorded retirement and a restored skill pass; an unresolvable
# base skips with a surfaced warning (never a silent pass).
set -euo pipefail

KASK_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GATE="$KASK_ROOT/scripts/check-retired-record.sh"
[ -f "$GATE" ] || { echo "FAIL: gate not found at $GATE"; exit 1; }

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
git -C "$fixture" init -q
git -C "$fixture" config user.email selftest@invalid
git -C "$fixture" config user.name selftest

mkdir -p "$fixture/.agents/skills/old" "$fixture/.agents/skills/gone" "$fixture/.agents/skills/back" "$fixture/crates/agent_skills"
printf 'old\n' > "$fixture/.agents/skills/old/SKILL.md"
printf 'gone\n' > "$fixture/.agents/skills/gone/SKILL.md"
printf 'back\n' > "$fixture/.agents/skills/back/SKILL.md"
cat > "$fixture/crates/agent_skills/agent_skills.rs" <<'EOF'
const RETIRED_SKILL_NAMES: &[&str] = &[
    "already-retired",
];
EOF
git -C "$fixture" add -A
git -C "$fixture" commit -qm base
base="$(git -C "$fixture" rev-parse HEAD)"

# Retire 'old' WITHOUT recording it; retire 'gone' WITH recording; delete
# and restore 'back' within the same commit (net: not retired).
rm -r "$fixture/.agents/skills/old" "$fixture/.agents/skills/gone" "$fixture/.agents/skills/back"
mkdir -p "$fixture/.agents/skills/back"
printf 'back\n' > "$fixture/.agents/skills/back/SKILL.md"
sed -i 's/"already-retired",/"already-retired",\n    "gone",/' "$fixture/crates/agent_skills/agent_skills.rs"
git -C "$fixture" add -A
git -C "$fixture" commit -qm retire
head="$(git -C "$fixture" rev-parse HEAD)"

failures=0

# Case 1: the unrecorded retirement fails, naming the skill.
set +e
out="$(REPO_ROOT="$fixture" bash "$GATE" --range "$base" "$head" 2>&1)"
rc=$?
set -e
if [ "$rc" -eq 0 ] || ! printf '%s\n' "$out" | grep -q "skill 'old'"; then
  echo "FAIL (case 1 — unrecorded retirement): rc=$rc"
  printf '%s\n' "$out" | tail -4
  failures=$((failures + 1))
else
  echo "OK (case 1 — unrecorded retirement): gate failed naming the missing skill"
fi

# Case 2: with 'old' recorded, the range passes (recorded + restored fine).
sed -i 's/"gone",/"gone",\n    "old",/' "$fixture/crates/agent_skills/agent_skills.rs"
git -C "$fixture" add -A
git -C "$fixture" commit -qm record
head2="$(git -C "$fixture" rev-parse HEAD)"
set +e
out="$(REPO_ROOT="$fixture" bash "$GATE" --range "$base" "$head2" 2>&1)"
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
  echo "FAIL (case 2 — recorded retirement): expected 0, got $rc"
  printf '%s\n' "$out" | tail -4
  failures=$((failures + 1))
else
  echo "OK (case 2 — recorded retirement + restored skill): gate exited 0"
fi

# Case 2b: a deletion-free range is a true zero, not a failure — the
# fail-closed capture must not conflate empty with error.
git -C "$fixture" commit -q --allow-empty -m empty
head3="$(git -C "$fixture" rev-parse HEAD)"
set +e
out="$(REPO_ROOT="$fixture" bash "$GATE" --range "$head2" "$head3" 2>&1)"
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
  echo "FAIL (case 2b — deletion-free range): expected 0, got $rc"
  printf '%s\n' "$out" | tail -4
  failures=$((failures + 1))
else
  echo "OK (case 2b — deletion-free range): a true zero passes"
fi

# Case 3: an unresolvable base skips with a surfaced warning, never a
# silent pass.
set +e
out="$(REPO_ROOT="$fixture" bash "$GATE" --range "0000000000000000000000000000000000000000" "$head2" 2>&1)"
rc=$?
set -e
if [ "$rc" -ne 0 ] || ! printf '%s\n' "$out" | grep -q "::warning::"; then
  echo "FAIL (case 3 — unresolvable base): expected surfaced skip, rc=$rc"
  printf '%s\n' "$out" | tail -4
  failures=$((failures + 1))
else
  echo "OK (case 3 — unresolvable base): surfaced skip with a warning annotation"
fi

if [ "$failures" -eq 0 ]; then
  echo "SELFTEST OK: retired-record gate is alive (unrecorded/recorded/deletion-free/unresolvable all pinned)"
  exit 0
fi
echo "SELFTEST FAIL: $failures case(s) did not behave as expected"
exit 1
