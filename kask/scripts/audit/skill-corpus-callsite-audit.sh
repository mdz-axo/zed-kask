#!/usr/bin/env bash
# Skill-corpus call-site audit — checks that a skill's SKILL.md documents
# at least one of the context variables its own-namespace templates
# declare, so a model following the SKILL.md can construct the render
# context without opening the template.
#
# For every skill in .agents/skills/ with a SKILL.md:
#   1. Extract backticked template references (`stem.j2`) from the body.
#   2. Resolve each against the skill's own registry namespace
#      (kask/registry/templates/<skill-name>/<stem>.j2). Cross-namespace
#      references (shared namespaces like company-research, media) are
#      skipped — the referencing skill is not the primary invoker.
#   3. For templates with an [inference] contract, extract the declared
#      input field names.
#   4. If ZERO of the declared inputs appear as words in the SKILL.md,
#      flag: callsite-blind — the model must open the template to know
#      what to pass.
#
# Known limitations (advisory, documented):
#   - Word matching is exact: a SKILL.md that describes the input in
#     prose without using the field name (e.g., "the learner's name"
#     instead of `learner_bot`) is flagged. Read-triage adjudicates.
#   - Cross-namespace templates are out of scope (the referencing skill
#     may be a methodology citation, not the invoker).
#   - Templates without contracts are skipped (the contract audit
#     covers their internal agreement; this checks the invocation seam).
#
# Advisory instrument: findings are proposals for read-triage, not edits.
# Usage: ./skill-corpus-callsite-audit.sh

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SKILLS="$SCRIPT_DIR/../../../.agents/skills"
REG="$SCRIPT_DIR/../../registry/templates"

checked=0
flagged=0
skipped=0
for skill_dir in "$SKILLS"/*/; do
    skill=$(basename "$skill_dir")
    skillmd="$skill_dir/SKILL.md"
    [ -f "$skillmd" ] || continue

    # Extract template stems (any .j2 mention — the [ -f ] resolution
    # below filters non-existent references automatically).
    for stem in $(grep -oE '[a-z0-9_-]+\.j2' "$skillmd" 2>/dev/null \
        | sed 's/\.j2$//' | sort -u || true); do
        template="$REG/$skill/$stem.j2"
        [ -f "$template" ] || continue
        grep -q '^  input:' "$template" || { skipped=$((skipped + 1)); continue; }

        checked=$((checked + 1))

        # Declared input field names (4-space indent, digits allowed,
        # nested sub-keys skipped — same extraction as the contract audit).
        ins=$(awk -v sect="  input:" '
            $0 == sect { insec = 1; next }
            insec && /^    [a-z_][a-z0-9_]*:/ { f = $1; sub(/:$/, "", f); print f; next }
            insec && /^      / { next }
            insec { insec = 0 }
        ' "$template")

        [ -z "$ins" ] && { skipped=$((skipped + 1)); continue; }

        # Count how many declared inputs appear as words in the SKILL.md.
        total=0
        documented=0
        for field in $ins; do
            total=$((total + 1))
            if grep -qw -- "$field" "$skillmd"; then
                documented=$((documented + 1))
            fi
        done

        if [ "$documented" -eq 0 ]; then
            echo "  $skill/$stem.j2: callsite-blind: $total declared inputs, none documented in SKILL.md"
            flagged=$((flagged + 1))
        fi
    done
done
echo "checked $checked call sites; $flagged flagged; $skipped skipped (no contract or unresolvable)"