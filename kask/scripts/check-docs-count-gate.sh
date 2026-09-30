#!/usr/bin/env bash
# Docs-count gate (DOCUMENTATION_STANDARDS.md §3): the kask/docs tree stays
# under the recorded cap with a working condensation target, the README's
# recorded corpus size matches the real count, and every gate reference
# carries the canonical cap/target numbers. The 2026-09-30 drift — 67
# recorded against a 73-file tree — landed undetected because the gate was
# documentation-enforced only; this script is the enforcement.
#
# Cap and target are parsed from the §3 canonical sentence, so an operator
# ruling that changes them needs no script edit — only the reference sweep
# the gate itself verifies. A reworded canonical sentence fails loudly here,
# never as a silent pass.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
DOCS_ROOT=${DOCS_ROOT:-"$root/kask/docs"}
DOCS_STANDARDS=${DOCS_STANDARDS:-"$root/kask/docs/architecture/DOCUMENTATION_STANDARDS.md"}
DOCS_README=${DOCS_README:-"$root/kask/docs/README.md"}

fail() {
    echo "FAIL: $1" >&2
    [ "${2:-}" ] && printf '      %s\n' "$2" >&2
    exit 1
}

[ -d "$DOCS_ROOT" ] || fail "docs root not found at $DOCS_ROOT" \
    "pass DOCS_ROOT (or restore kask/docs) — an unscannable root fails loudly, never as a silent pass"
[ -f "$DOCS_STANDARDS" ] || fail "DOCUMENTATION_STANDARDS.md not found at $DOCS_STANDARDS"
[ -f "$DOCS_README" ] || fail "kask/docs/README.md not found at $DOCS_README"

# ── Policy: cap and target from the §3 canonical sentence ─────────────────
cap=$(sed -n 's/.*capped at \*\*fewer than \([0-9]\{1,\}\) files\*\*.*/\1/p' "$DOCS_STANDARDS" | head -1)
target=$(sed -n 's/.*working \*\*target of \([0-9]\{1,\}\) files\*\*.*/\1/p' "$DOCS_STANDARDS" | head -1)
[ "$cap" ] || fail "cannot parse the cap from $DOCS_STANDARDS" \
    "the §3 canonical sentence ('capped at **fewer than N files**') was reworded or moved — restore the phrasing or update the regex in kask/scripts/check-docs-count-gate.sh; a gate that cannot parse its policy fails loudly, never as a silent pass"
[ "$target" ] || fail "cannot parse the working target from $DOCS_STANDARDS" \
    "the §3 canonical sentence ('working **target of N files**') was reworded or moved — restore the phrasing or update the regex in kask/scripts/check-docs-count-gate.sh"

# ── Count and hard cap ────────────────────────────────────────────────────
actual=$(find "$DOCS_ROOT" -type f | wc -l | tr -d ' ') || fail "scanning $DOCS_ROOT failed"
if (( actual >= cap )); then
    fail "kask/docs holds $actual files — at or over the fewer-than-$cap cap (DOCUMENTATION_STANDARDS.md §3)" \
    "fold or delete a leaf document per §3 (recording its successor in the README lifecycle ledger), or raise the cap by operator ruling — updating every gate reference in the same change"
fi

# ── Recorded-measurement parity (the drift detector) ──────────────────────
corpus_line=$(grep -m1 '^\*\*Corpus size (measured' "$DOCS_README" || true)
[ "$corpus_line" ] || fail "README corpus-size measurement line not found" \
    "the '**Corpus size (measured ...)**' anchor in kask/docs/README.md is missing or reworded — restore it or update the anchor in check-docs-count-gate.sh"
recorded_corpus=$(printf '%s\n' "$corpus_line" | sed -n 's/.* \([0-9]\{1,\}\) files total.*/\1/p')
[ "$recorded_corpus" ] || fail "cannot parse the recorded file count from the README corpus-size line" \
    "the 'N files total' phrase is missing — restore it or update the regex in check-docs-count-gate.sh"
[ "$recorded_corpus" = "$actual" ] || fail "README corpus-size line records $recorded_corpus files total but the tree holds $actual" \
    "update the '**Corpus size (measured ...)**' line in kask/docs/README.md in the same change that adds or removes docs files"

checklist_line=$(grep -m1 'Document count is' "$DOCS_README" || true)
[ "$checklist_line" ] || fail "README verification-gate checklist entry not found" \
    "the 'Document count is ...' line in kask/docs/README.md is missing or reworded — restore it or update the anchor in check-docs-count-gate.sh"
recorded_checklist=$(printf '%s\n' "$checklist_line" | sed -n 's/.*Document count is \([0-9]\{1,\}\).*/\1/p')
[ "$recorded_checklist" ] || fail "cannot parse the recorded count from the README checklist entry" \
    "the 'Document count is N' phrase is missing — restore it or update the regex in check-docs-count-gate.sh"
[ "$recorded_checklist" = "$actual" ] || fail "README checklist records document count $recorded_checklist but the tree holds $actual" \
    "update the 'Document count is ...' verification-gate entry in the same change that adds or removes docs files"

# ── Gate-reference agreement (cap/target consistent everywhere) ───────────
standards_checklist_line=$(grep -m1 'Document-count gate:' "$DOCS_STANDARDS" || true)
[ "$standards_checklist_line" ] || fail "STANDARDS §3 authoring-checklist gate item not found" \
    "the 'Document-count gate:' line in DOCUMENTATION_STANDARDS.md is missing or reworded — restore it or update the anchor in check-docs-count-gate.sh"
standards_bash_line=$(grep -m1 'must stay <' "$DOCS_STANDARDS" || true)
[ "$standards_bash_line" ] || fail "STANDARDS §B.6 bash example not found" \
    "the 'must stay < ...' line in DOCUMENTATION_STANDARDS.md §B.6 is missing or reworded — restore it or update the anchor in check-docs-count-gate.sh"

check_ref_carries() {
    local where=$1 line=$2
    printf '%s\n' "$line" | grep -qw "$cap" || fail "gate reference divergence: the $where does not carry the canonical cap ($cap)" \
        "update every gate reference (README corpus-size line, README checklist, STANDARDS §3 checklist, STANDARDS §B.6 bash example) in the same change that alters the cap or target"
    printf '%s\n' "$line" | grep -qw "$target" || fail "gate reference divergence: the $where does not carry the canonical working target ($target)" \
        "update every gate reference (README corpus-size line, README checklist, STANDARDS §3 checklist, STANDARDS §B.6 bash example) in the same change that alters the cap or target"
}
check_ref_carries "README corpus-size line" "$corpus_line"
check_ref_carries "README checklist line" "$checklist_line"
check_ref_carries "STANDARDS §3 checklist" "$standards_checklist_line"
check_ref_carries "STANDARDS §B.6 bash example" "$standards_bash_line"

# ── Working target: condensation direction, not a gate ────────────────────
if (( actual > target )); then
    printf 'note: %s files exceeds the %s-file working target by %s — condensation direction (DOCUMENTATION_STANDARDS.md §3), not a gate\n' \
        "$actual" "$target" "$((actual - target))"
fi

printf 'docs count gate: %s files under the fewer-than-%s cap (working target %s) — README measurement current, gate references consistent\n' \
    "$actual" "$cap" "$target"
