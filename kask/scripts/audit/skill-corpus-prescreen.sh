#!/usr/bin/env bash
# Skill-corpus prescreen — the mechanical layer of the skill-logic-audit
# corpus pass, and the embryo of the body-side validator.
#
# For every shipped .j2 template under kask/registry/templates/:
#   1. Goal presence — the `{# goal: ... #}` annotation that
#      skill-logic-audit's logic-load-goal step parses.
#   2. Goal length sanity — a placeholder goal is not a goal.
#   3. Goal-content overlap — the fraction of the goal's content words
#      present in the template body (comments stripped). Low overlap
#      flags a transcription mismatch (wrong goal on the wrong template)
#      for read-triage.
#   4. Goal-wrap detection — a goal spread across consecutive
#      {# ... #} blocks parses as only its first line (mid-phrase).
#      Flags first-block goals lacking terminal punctuation for
#      read-triage (a complete unpunctuated goal is a benign flag).
#
# Advisory instrument: findings are proposals for read-triage, not edits.
# Usage: ./skill-corpus-prescreen.sh [overlap_floor]   (default 0.25)

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REG="$SCRIPT_DIR/../../registry/templates"
OVERLAP_FLOOR="${1:-0.25}"

STOPWORDS='^(the|a|an|and|or|of|to|for|from|with|into|on|in|by|is|are|be|as|not|no|its|it|this|that|which|who|what|how|when|where|why|you|your|we|our|they|their|all|any|each|per|via|using|use|used|must|only|one|two)$'

# Extract the {# goal: ... #} annotation (single- or multi-line).
extract_goal() {
    awk '
        !done && /[{]#[[:space:]]*[Gg]oal:/ {
            line = $0
            sub(/.*[{]#[[:space:]]*[Gg]oal:[[:space:]]*/, "", line)
            if (line ~ /#}/) { sub(/#}.*/, "", line); print line; done = 1; exit }
            buf = line; ingoal = 1; next
        }
        ingoal {
            line = $0
            if (line ~ /#}/) { sub(/#}.*/, "", line); print buf " " line; exit }
            buf = buf " " line
        }' "$1"
}

# Strip Jinja {# ... #} comment blocks (inline and multi-line).
strip_comments() {
    awk '
        {
            line = $0; out = ""
            while (length(line) > 0) {
                if (!inc) {
                    p = index(line, "{#")
                    if (p > 0) { out = out substr(line, 1, p - 1); line = substr(line, p + 2); inc = 1 }
                    else { out = out line; line = "" }
                } else {
                    q = index(line, "#}")
                    if (q > 0) { line = substr(line, q + 2); inc = 0 }
                    else { line = "" }
                }
            }
            print out
        }' "$1"
}

checked=0
flagged=0
for file in "$REG"/*/*.j2; do
    checked=$((checked + 1))
    rel="${file#"$REG"/}"
    goal="$(extract_goal "$file" || true)"
    if [ -z "$goal" ]; then
        echo "  $rel: NO GOAL |"
        flagged=$((flagged + 1))
        continue
    fi
    if [ "${#goal}" -lt 10 ]; then
        echo "  $rel: SHORT GOAL | $goal"
        flagged=$((flagged + 1))
        continue
    fi
    # Goal-wrap detection: the canonical parse (extract_goal above and
    # skill-logic-audit's logic-load-goal) reads ONE {# goal: ... #}
    # block. A goal wrapped across consecutive blocks parses as only
    # its first line, which ends mid-phrase.
    case "$goal" in
        *[.!?]*) ;;
        *)
            echo "  $rel: GOAL WRAP? | ${goal:0:90}"
            flagged=$((flagged + 1))
            continue
            ;;
    esac
    body="$(strip_comments "$file")"
    # Grep reads the body from a herestring (a temp file), not a pipe:
    # grep -q exits at the first match, and an early-exiting reader on a
    # pipe SIGPIPEs the writer — pipefail then drops a counted hit and
    # can flip a borderline template's overlap below the floor.
    total=0
    hits=0
    for word in $(printf '%s' "$goal" | tr '[:upper:]' '[:lower:]' \
        | grep -oE '[a-z_][a-z0-9_-]{2,}' | sort -u \
        | grep -vE "$STOPWORDS" || true); do
        total=$((total + 1))
        if grep -qiw -- "$word" <<< "$body"; then
            hits=$((hits + 1))
        fi
    done
    if [ "$total" -gt 0 ]; then
        overlap=$(awk -v h="$hits" -v t="$total" 'BEGIN { printf "%.0f", 100 * h / t }')
        if awk -v o="$overlap" -v f="$OVERLAP_FLOOR" 'BEGIN { exit !(o < f * 100) }'; then
            echo "  $rel: LOW OVERLAP ${overlap}% | ${goal:0:90}"
            flagged=$((flagged + 1))
        fi
    fi
done
echo "checked $checked templates; $flagged flagged"
