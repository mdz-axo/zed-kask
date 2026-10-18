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
