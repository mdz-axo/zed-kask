#!/usr/bin/env bash
# Skill-corpus call-site audit — for every template a SKILL.md references,
# in its own namespace or another skill's, check that the SKILL.md names
# every REQUIRED input of that template's [inference] contract, so a model
# following the SKILL.md can construct the render context without opening
# the template.
#
# For every skill in .agents/skills/ with a SKILL.md:
#   1. Extract backticked template references from the body:
#      `stem.j2` (own namespace) and `namespace/stem` or `namespace/stem.j2`
#      (any namespace — a cross-skill delegation is a call site too; the
#      2026-11-12 essentialist→coding-guidelines/guidelines-verify case
#      was a delegation whose required inputs the caller never held).
#   2. Resolve each against kask/registry/templates/. Unresolvable refs
#      are skipped (the crossrefs check owns dangling references).
#   3. Read the contract's required inputs: every `input:` key except
#      those typed `…|null`, carrying `required: false`, or carrying a
#      `default:`. Nested sub-keys are not inputs.
#   4. Flag each required input that does not appear as a word in the
#      SKILL.md: `callsite-gap`. Zero documented inputs is reported as
#      `callsite-blind`.
#
# Known limitations (advisory, documented):
#   - Word matching is exact: a SKILL.md that describes an input in prose
#     without the field name is flagged. A reference that is a pattern or
#     methodology citation, not an invocation, is accepted by listing it
#     in skill-corpus-callsite-accepted.txt as `<skill> <namespace/stem>`.
#   - Templates without contracts are skipped (the contract audit covers
#     their internal agreement; this checks the invocation seam).
#
# Exit status: 0 when nothing is flagged, 1 otherwise (the selftest and
# the skill-maintenance checklist read the exit).
# Usage: ./skill-corpus-callsite-audit.sh

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SKILLS="${SKILL_CORPUS_DIR:-$SCRIPT_DIR/../../../.agents/skills}"
REG="${TEMPLATE_CORPUS_DIR:-$SCRIPT_DIR/../../registry/templates}"
ACCEPTED="${CALLSITE_ACCEPTED_FILE:-$SCRIPT_DIR/skill-corpus-callsite-accepted.txt}"

checked=0
flagged=0
skipped=0
accepted=0
for skill_dir in "$SKILLS"/*/; do
    skill=$(basename "$skill_dir")
    skillmd="$skill_dir/SKILL.md"
    [ -f "$skillmd" ] || continue

    # Own-namespace `stem.j2` and any-namespace `ns/stem(.j2)` references,
    # normalised to `ns/stem`.
    refs=$( {
        grep -oE '`[a-z0-9_-]+\.j2`' "$skillmd" 2>/dev/null \
            | tr -d '`' | sed "s/\.j2\$//; s#^#$skill/#" || true
        grep -oE '`[a-z0-9_-]+/[a-z0-9_-]+(\.j2)?`' "$skillmd" 2>/dev/null \
            | tr -d '`' | sed 's/\.j2$//' || true
    } | sort -u)

    for ref in $refs; do
        template="$REG/$ref.j2"
        [ -f "$template" ] || continue
        grep -q '^  input:' "$template" || { skipped=$((skipped + 1)); continue; }

        if [ -f "$ACCEPTED" ] && grep -qxF -- "$skill $ref" "$ACCEPTED"; then
            accepted=$((accepted + 1))
            continue
        fi
        checked=$((checked + 1))

        # Required input names: 4-space-indented keys under `  input:`,
        # minus `|null` types, `required: false` and `default:` carriers.
        required=$(awk '
            $0 == "  input:" { insec = 1; next }
            insec && /^    [a-z_][a-z0-9_]*:/ {
                flush()
                name = $1; sub(/:$/, "", name)
                rest = $0; sub(/^    [a-z_][a-z0-9_]*:[ ]*/, "", rest)
                optional = (rest ~ /\|[ ]*null/) ? 1 : 0
                next
            }
            insec && /^      / {
                if ($0 ~ /^      type:.*\|[ ]*null/) optional = 1
                if ($0 ~ /^      required:[ ]*false/) optional = 1
                if ($0 ~ /^      default:/) optional = 1
                next
            }
            insec { flush(); insec = 0 }
            function flush() { if (name != "" && !optional) print name; name = ""; optional = 0 }
            END { flush() }
        ' "$template")

        [ -z "$required" ] && continue

        total=0
        missing=""
        for field in $required; do
            total=$((total + 1))
            grep -qw -- "$field" "$skillmd" || missing="$missing $field"
        done
        gap=$(echo "$missing" | wc -w)
        if [ "$gap" -eq "$total" ]; then
            echo "  $skill -> $ref: callsite-blind: $total required inputs, none documented in SKILL.md:$missing"
            flagged=$((flagged + 1))
        elif [ "$gap" -gt 0 ]; then
            echo "  $skill -> $ref: callsite-gap: $gap of $total required inputs undocumented in SKILL.md:$missing"
            flagged=$((flagged + 1))
        fi
    done
done
echo "checked $checked call sites; $flagged flagged; $accepted accepted (baseline); $skipped skipped (no contract)"
[ "$flagged" -eq 0 ]
