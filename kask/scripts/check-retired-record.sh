#!/usr/bin/env bash
# CI gate: every skill retired within the change range must be recorded in
# RETIRED_SKILL_NAMES (crates/agent_skills/agent_skills.rs) in the same
# change — the DIVERGENCE.md D1 retirement convention ("the retirement
# change appends its name there in the same commit"). A forgotten append
# silently shrinks BOTH the installed-copy sweep and the skill-crossrefs
# gate (which derives its former-name list from the record), so the
# omission fails here instead.
#
# usage: check-retired-record.sh --range BASE HEAD
# Exit codes:
#   0 — every retirement in range is recorded (or none occurred; or the
#       range is unavailable — surfaced as a ::warning, never a silent pass)
#   1 — a retirement in range is missing from the record
#   2 — usage error
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
RECORD="$REPO_ROOT/crates/agent_skills/agent_skills.rs"

if [ "${1:-}" != "--range" ] || [ "$#" -ne 3 ]; then
  echo 'usage: check-retired-record.sh --range BASE HEAD' >&2
  exit 2
fi
base="$2"
head="$3"

[ -f "$RECORD" ] || { echo "FAIL: retired-skill record not found at $RECORD" >&2; exit 1; }

if ! git -C "$REPO_ROOT" rev-parse --verify --quiet "${base}^{commit}" >/dev/null 2>&1; then
  # A force-push or a brand-new ref leaves no resolvable base. Surface the
  # skip — never a silent pass: the annotation is visible in the run.
  echo "::warning::retired-record completeness skipped — base ${base} is not resolvable (force-push or new ref); retirements in this range are unverified."
  exit 0
fi
git -C "$REPO_ROOT" rev-parse --verify --quiet "${head}^{commit}" >/dev/null 2>&1 || {
  echo "FAIL: head ${head} is not resolvable" >&2
  exit 1
}

missing=0
# Fail-closed input capture: the deletion list is captured under set -e so
# a git failure propagates — a process substitution would discard the exit
# status, and an empty read would be indistinguishable from "no
# retirements" (the vacuous-pass class this gate exists to prevent). An
# empty capture here is a true zero, never a failure.
retired_names="$(git -C "$REPO_ROOT" diff --name-only --diff-filter=D "$base" "$head" -- '.agents/skills/*/SKILL.md' \
  | sed -e 's|^\.agents/skills/||' -e 's|/SKILL\.md$||')"
while IFS= read -r name; do
  [ -n "$name" ] || continue
  # A skill restored by HEAD is not retired — the record holds only names
  # absent from HEAD.
  [ -e "$REPO_ROOT/.agents/skills/$name/SKILL.md" ] && continue
  if ! grep -qF "\"$name\"" "$RECORD"; then
    echo "FAIL: skill '$name' was retired in ${base}..${head} but is not in RETIRED_SKILL_NAMES ($RECORD)." >&2
    echo "      The retirement change must append the name in the same commit (DIVERGENCE.md D1)." >&2
    missing=1
  fi
done <<< "$retired_names"

if [ "$missing" -ne 0 ]; then
  exit 1
fi
echo "OK: every skill retired in ${base}..${head} is recorded in RETIRED_SKILL_NAMES."
