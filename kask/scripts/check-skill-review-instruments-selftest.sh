#!/usr/bin/env bash
# Selftest for the two audit instruments the 58-skill semantic review cites
# as evidence (skill-review-anchors.sh, skill-review-doc-cites.sh — both under
# kask/scripts/audit/). Both instruments were defective on 2026-10-18 (the
# anchors pattern over-matched, reading prose as an ontology citation; both
# exited 1 on a clean sweep via grep's no-match exit under pipefail), and
# both were repaired and validated with injected-violation controls. This
# selftest pins the repaired behavior so neither defect can silently return:
# the two instruments must exit 0 on the live corpus and must exit nonzero
# on an injected dangling citation.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ANCHORS="$repo_root/kask/scripts/audit/skill-review-anchors.sh"
DOC_CITES="$repo_root/kask/scripts/audit/skill-review-doc-cites.sh"

for inst in "$ANCHORS" "$DOC_CITES"; do
  [ -f "$inst" ] || { echo "FAIL: missing instrument $inst" >&2; exit 1; }
done

# ── Positive: both instruments exit 0 on the live corpus ──
if ! out="$(bash "$ANCHORS" 2>&1)"; then
  echo "FAIL: anchors instrument failed on the live corpus: $out" >&2
  exit 1
fi
[[ "$out" == *'0 dangling'* ]] || {
  echo "FAIL: anchors instrument did not report 0 dangling on the live corpus: $out" >&2
  exit 1
}

if ! out="$(bash "$DOC_CITES" 2>&1)"; then
  echo "FAIL: doc-cites instrument failed on the live corpus: $out" >&2
  exit 1
fi
[[ "$out" == *'0 missing'* ]] || {
  echo "FAIL: doc-cites instrument did not report 0 missing on the live corpus: $out" >&2
  exit 1
}

# ── Negative: an injected dangling derived citation must exit nonzero ──
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/kask/scripts/audit" \
         "$fixture/kask/crates/hkask-bridge-ontology/src" \
         "$fixture/.agents/skills/probe-skill"
cp "$ANCHORS" "$DOC_CITES" "$fixture/kask/scripts/audit/"
# A derived.rs containing one known key, so any other key dangles.
printf 'pub const X: &str = "deep_module";\n' \
  > "$fixture/kask/crates/hkask-bridge-ontology/src/derived.rs"
# A skill citing the known key (passes) and an unknown key (must fail).
printf '%s\n' \
  'Grounded in `onto_anchor` → derived `deep_module`.' \
  'Also cites `onto_anchor` → derived `no_such_derived_key`.' \
  > "$fixture/.agents/skills/probe-skill/SKILL.md"

if bash "$fixture/kask/scripts/audit/skill-review-anchors.sh" >/dev/null 2>&1; then
  echo "FAIL: anchors instrument accepted a dangling derived citation" >&2
  exit 1
fi

# The false-positive direction: prose about something "EQM-derived" (no
# onto_anchor context) must NOT be read as an ontology citation.
printf '%s\n' \
  'The optional EQM-derived `overconfidence_bias` input.' \
  > "$fixture/.agents/skills/probe-skill/SKILL.md"
if ! bash "$fixture/kask/scripts/audit/skill-review-anchors.sh" >/dev/null 2>&1; then
  echo "FAIL: anchors instrument read prose as an ontology citation (the 2026-10-18 over-match defect)" >&2
  exit 1
fi

# ── Negative: a missing cited doc must exit nonzero ──
printf '%s\n' \
  'See kask/docs/reference/this-doc-does-not-exist.md for details.' \
  > "$fixture/.agents/skills/probe-skill/SKILL.md"
if bash "$fixture/kask/scripts/audit/skill-review-doc-cites.sh" >/dev/null 2>&1; then
  echo "FAIL: doc-cites instrument accepted a citation to a nonexistent doc" >&2
  exit 1
fi

echo 'OK: the skill-review audit instruments pass the live corpus, reject dangling citations, reject missing docs, and do not over-match prose.'

# ── The retirement-record instruments ──
# derive-retired-skill-gap.sh re-derives the retirement set by the record's own
# stated method and diffs it against the curated array; a record missing a
# retired-and-shipped name must be reported as a gap, and a complete record must
# report none. check-retired-shipped-status.sh reports whether a named retired
# skill ever shipped (and so needs a sweep entry).
GAP="$repo_root/kask/scripts/audit/derive-retired-skill-gap.sh"
SHIPPED="$repo_root/kask/scripts/audit/check-retired-shipped-status.sh"
for inst in "$GAP" "$SHIPPED"; do
  [ -f "$inst" ] || { echo "FAIL: missing instrument $inst" >&2; exit 1; }
  bash -n "$inst" || { echo "FAIL: $inst has a syntax error" >&2; exit 1; }
done

# Positive: the live record reconciles — no name retired-and-shipped is absent
# from the curated array.
gap_out="$(bash "$GAP" 2>&1)"
printf '%s\n' "$gap_out" | grep -q 'curated:' || {
  echo "FAIL: gap instrument did not report counts: $gap_out" >&2
  exit 1
}
printf '%s\n' "$gap_out" | sed -n '/MISSING from the curated record/,$p' \
  | tail -n +2 | grep -qE '^[a-z0-9-]+$' && {
  echo "FAIL: the live retirement record still has a gap: $gap_out" >&2
  exit 1
}

# Negative: a name that never shipped is reported as needing no sweep entry,
# and one that shipped is reported as requiring one.
never="$(bash "$SHIPPED" lora-training 2>&1 || true)"
printf '%s\n' "$never" | grep -q 'sweep entry REQUIRED' || {
  echo "FAIL: shipped-status instrument did not flag a shipped retired skill: $never" >&2
  exit 1
}

# Negative: a name with no deletion commit is reported, not silently passed.
missing="$(bash "$SHIPPED" no-such-skill-ever 2>&1 || true)"
printf '%s\n' "$missing" | grep -q 'no deletion commit found' || {
  echo "FAIL: shipped-status instrument did not report an unknown name: $missing" >&2
  exit 1
}

echo 'OK: the retirement-record instruments reconcile the live record and flag a shipped retired name.'
