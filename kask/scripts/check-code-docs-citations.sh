#!/usr/bin/env bash
# check-code-docs-citations.sh — code→docs citation gate (advisory).
#
# Companion to check-docs-citations.sh (which scopes the citations INSIDE
# kask/docs). This gate sweeps every live surface OUTSIDE kask/docs —
# code, templates, scripts, skills, agent-guidance docs — for
# `kask/docs/**.md` path citations and fails when a cited doc does not
# exist: a live tool description or code comment pointing at a deleted
# doc is active misinformation shipped to every session (the 2026-11-01
# alignment audit's P1 class: the kanban tool descriptions cited
# kanban-board-reference-models.md for two days after the docs
# condensation deleted it).
#
# Historical-record scopes stay WARNINGS, not failures: DIVERGENCE.md rows
# record seam provenance ("created per the plan that ..."), and past-tense
# mentions that name a deletion are honest records, not live pointers.
#
# Deliberate negative-test fixtures are allowlisted — each entry a
# recorded decision (the check-no-python.sh allowlist pattern).
#
# ADVISORY AGENT TOOL — unwired from push/CI enforcement by operator
# mandate 2026-10-31 (the rules-that-don't-improve-code-quality cut),
# consistent with the sibling citation gates. Run it voluntarily;
# re-wiring it requires an explicit operator decision.
#
# Usage: bash kask/scripts/check-code-docs-citations.sh
# Env:   CODE_CITES_ROOT — root override for the selftest (defaults to
#        the git repo root).
# Exit codes: 0 = no rotted live-surface citations (warnings allowed),
#             1 = rotted live-surface citations found,
#             2 = usage/tooling error.

set -uo pipefail

ROOT="${CODE_CITES_ROOT:-}"
if [ -z "$ROOT" ]; then
    ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || {
        echo "FAIL: not inside a git repository (run from within the repo, or set CODE_CITES_ROOT)." >&2
        exit 2
    }
fi
[ -d "$ROOT" ] || { echo "FAIL: root does not exist: $ROOT" >&2; exit 2; }
cd "$ROOT" || exit 2
command -v rg >/dev/null 2>&1 || { echo "FAIL: rg not found" >&2; exit 2; }

# Deliberate fixtures (each a recorded decision):
# - check-skill-review-instruments-selftest.sh cites
#   kask/docs/reference/this-doc-does-not-exist.md as its negative fixture.
# - This gate and its own selftest carry cite-path strings as fixtures
#   and documentation (the self-reference class): sweeping them would
#   flag the very fixture data that pins the gate.
ALLOWLIST_RE='(^|/)(check-code-docs-citations(-selftest)?|check-skill-review-instruments-selftest)\.sh$'

# Historical-record emitters: provenance mentions, not live pointers.
HISTORICAL_RE='(^|/)DIVERGENCE\.md$'

ok=0; fail=0; warn=0
# file<TAB>cited-path pairs from every live surface outside kask/docs.
while IFS="$(printf '\t')" read -r f c; do
    case "$f" in
        kask/docs/*) continue ;;   # docs-internal citations: the sibling gate's scope
    esac
    if printf '%s\n' "$f" | grep -Eq "$ALLOWLIST_RE"; then continue; fi
    if printf '%s\n' "$f" | grep -Eq "$HISTORICAL_RE"; then hist=1; else hist=0; fi
    if [ ! -e "$ROOT/$c" ]; then
        if [ "$hist" -eq 1 ]; then
            echo "warn: $f cites deleted $c (historical-record scope: provenance)" >&2
            warn=$((warn+1))
        else
            echo "::error::code-docs-citation: $f cites deleted $c (restore the doc or excise the citation)" >&2
            fail=$((fail+1))
        fi
    else
        ok=$((ok+1))
    fi
done < <(rg -o --no-line-number --hidden \
        'kask/docs/[A-Za-z0-9_./-]+\.md' \
        -g '!kask/docs' -g '!.git' -g '!target' 2>/dev/null \
        | awk -F: '{ split($0, a, ":"); print a[1] "\t" a[2] }')

echo "----"
echo "code->docs citations: $ok resolved, $fail rotted (failures), $warn historical-record warnings"
if [ "$fail" -eq 0 ]; then exit 0; fi
exit 1
