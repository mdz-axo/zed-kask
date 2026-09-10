#!/usr/bin/env bash
# Skill-corpus S9/S10 sweep — mechanical enforcement point for the
# skill-maintenance validator's frontmatter and removed-vocabulary checks.
#
# S9: no `visibility` field in SKILL.md frontmatter.
# S10: removed manifest-executor vocabulary as *dispatch structure*:
#   - frontmatter keys: visibility / steps / compute_ref /
#     convergence_signal / input_mapping / on_failure / ordinal /
#     action / template_ref (frontmatter is structural — no live-contract
#     reading exists there; a `steps:` key is the vestigial executor remnant)
#   - body key-form lines of the tokens with NO live contract
#     (compute_ref, convergence_signal, input_mapping, on_failure, ordinal:)
#     anywhere in the body, outside frontmatter
#   `template_ref` / `action` have live contracts (the render_template
#   parameter; tool parameters), so body key-form usages are NOT flagged
#   here — S10's carve-out adjudicates them at read-triage.
#
# Advisory instrument: findings are proposals for read-triage, not edits.
# Usage: ./skill-corpus-s9-s10-sweep.sh

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SKILLS="$SCRIPT_DIR/../../../.agents/skills"

checked=0
flagged=0
for file in "$SKILLS"/*/SKILL.md; do
    checked=$((checked + 1))
    rel="${file#"$SKILLS"/}"
    hits=$(awk -v rel="$rel" '
        NR==1 { infm = ($0 == "---"); next }
        infm && $0 == "---" { infm = 0; next }
        infm && /^[[:space:]]*-?[[:space:]]*(visibility|steps|compute_ref|convergence_signal|input_mapping|on_failure|ordinal|action|template_ref)[[:space:]]*:/ {
            print "  " rel ": FRONTMATTER " $0
            next
        }
        !infm && /^[[:space:]]*-?[[:space:]]*(compute_ref|convergence_signal|input_mapping|on_failure|ordinal)[[:space:]]*:/ {
            print "  " rel ": BODY " $0
        }' "$file")
    if [ -n "$hits" ]; then
        echo "$hits"
        flagged=$((flagged + 1))
    fi
done
echo "checked $checked SKILL.mds; $flagged flagged"
