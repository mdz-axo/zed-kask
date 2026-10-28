#!/usr/bin/env bash
# Self-test for the commit-msg guard (kask/githooks/commit-msg).
#
# Institutionalizes the .rules trap "A CI gate must be shown to fail before
# its status: enforced is trusted". All three observed artifact classes are
# pinned, each with the live incident as its case:
#
#   1. Markdown fences — bare first/last fence line (97 commits on main,
#      2026-07-23..2026-09-09) and the inline-fenced subject shape
#      (33240e9109: "``` Remove grounding enforcement ... ```").
#   2. Placeholder subjects — an agent meta-response leaked as the
#      message (c86faf6a59: "No changes were provided — ...").
#   3. Mail-header subjects — an email-style "Subject:" header passed
#      through as the message (b15bfb86f7: 'Subject: "Re-verify docs and
#      remove bug-hunt report"').
#
# Plus the pass-through case: an ordinary subject and body must be
# accepted (the guard must not start rejecting real messages).
#
# Exit codes:
#   0 — every case behaved as expected (the guard is alive)
#   1 — at least one case did not behave as expected (guard broken or vacuous)
#
# Usage: bash kask/scripts/check-commit-msg-selftest.sh

set -euo pipefail

KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOOK="$KASK_ROOT/githooks/commit-msg"

if [ ! -f "$HOOK" ]; then
  echo "FAIL: hook not found at $HOOK"
  exit 1
fi

failures=0

# expect <description> <expected-exit-0-or-1> <message-content>
expect() {
  local description="$1" expected_exit="$2" content="$3"
  local tmp
  tmp="$(mktemp)"
  printf '%s\n' "$content" > "$tmp"
  local exit_code=0
  bash "$HOOK" "$tmp" >/dev/null 2>&1 || exit_code=$?
  rm -f "$tmp"
  if [ "$exit_code" -eq "$expected_exit" ]; then
    echo "PASS: $description"
  else
    echo "FAIL: $description — expected exit $expected_exit, got $exit_code" >&2
    failures=$((failures + 1))
  fi
}

# ── Rejection classes ───────────────────────────────────────────────────────

expect "bare leading fence rejected" 1 '```
Real subject inside fences'

expect "bare trailing fence rejected" 1 'Real subject

Body line.

```'

expect "inline-fenced subject rejected (33240e9109 class)" 1 '``` Remove grounding enforcement from MCP servers ```'

expect "placeholder subject rejected (c86faf6a59 class)" 1 \
'No changes were provided — the diff content after "Here are the changes in this commit:" is missing. Please share the diff so I can write an accurate commit message.'

expect "meta-preamble subject rejected" 1 'Here are the changes in this commit:'

expect "refusal subject rejected" 1 "I cannot write a commit message without the diff."

expect "apology subject rejected" 1 "I'm sorry, but no diff was provided."

expect "mail-header subject rejected (b15bfb86f7 class)" 1 'Subject: "Re-verify docs and remove bug-hunt report"'

# ── Pass-through ────────────────────────────────────────────────────────────

expect "ordinary subject and body accepted" 0 'Fix board name cap, typed errors, sheet state

Body explaining the change.'

expect "subject mentioning fences in prose accepted" 0 \
'Strip bare fence lines before committing'

expect "subject naming a subject line in prose accepted" 0 \
'Subject line hygiene in the commit-msg guard'

# ── Goal-traceability (advisory; invoked by the hook) ────────────────────────
# The fixed script must recognize a convention-placed Goal line (after the
# subject — `git interpret-trailers --parse` sees only the final paragraph,
# which made every convention-placed Goal line invisible and every compliant
# commit a false ADVISORY until 2026-10-01). A compliant message must never
# produce the "missing or malformed" ADVISORY, whatever is staged.
traceability_msg="$(mktemp)"
traceability_out="$(mktemp)"
printf 'Repair the skill\n\nGoal: 708d6e58-03df-4dcd-ab13-1a778fedd580\n\nBody.\n' > "$traceability_msg"
bash "$KASK_ROOT/scripts/check-goal-traceability.sh" --message "$traceability_msg" > "$traceability_out" 2>&1
if grep -q 'missing or malformed' "$traceability_out"; then
  echo "FAIL: convention-placed Goal line (after the subject) still false-fires the ADVISORY" >&2
  failures=$((failures + 1))
else
  echo "PASS: convention-placed Goal line recognized (no false ADVISORY)"
fi
rm -f "$traceability_msg" "$traceability_out"

if [ "$failures" -eq 0 ]; then
  echo "commit-msg selftest: all cases passed"
  exit 0
fi

echo "commit-msg selftest: $failures case(s) failed" >&2
exit 1