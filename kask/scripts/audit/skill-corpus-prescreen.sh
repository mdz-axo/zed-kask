#!/usr/bin/env bash
# Skill-corpus prescreen — the mechanical layer of skill-maintenance's template-logic audit
# corpus pass, and the embryo of the body-side validator.
#
# For every .j2 template under kask/registry/templates/:
#   1. Goal presence — the `{# goal: ... #}` annotation that
#      the template-logic audit's logic-load-goal step parses.
#   2. Goal length sanity — a placeholder goal is not a goal.
#   3. Goal-content overlap — the fraction of the goal's content words
#      present in the template body (comments stripped). Low overlap
#      flags a transcription mismatch (wrong goal on the wrong template)
#      for read-triage.
#   4. Goal-wrap detection — a goal spread across consecutive
#      {# ... #} blocks parses as only its first line (mid-phrase).
#      Flags first-block goals lacking terminal punctuation for
#      read-triage (a complete unpunctuated goal is a benign flag).
#   5. D/P labelling presence (body-side, P8.4) — every
#      computation-prescribing SKILL.md (it names an oracle: lisp_eval,
#      lean_check, cargo — test/check/clippy/bench/tree — criterion,
#      scenario_* tool calls) carries a D/P labelling
#      section. Presence is the audit floor; the routing correctness of
#      any label is judgment (P), critiqued in review. Codified here
#      2026-09-28 after the filesystem walk ran repeatedly as a re-typed
#      one-liner across the convergence batches (12 → 55 of 55).
#   6. Label-vs-tool consistency (body-side, P8.4) — a SKILL.md that
#      names a D oracle must assert a D regime somewhere in the file
#      (the conservative whole-file floor of the doctrine's
#      "checkably wrong" example; per-step routing correctness stays
#      P). Codified 2026-10-08 from the D/P audit's hand-run
#      label-vs-tool sweep.
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
accepted=0
BASELINE="$SCRIPT_DIR/skill-corpus-prescreen-accepted.txt"

# Report a flag unless it is a read-triaged acceptance in the baseline
# file. Baseline entries are `<rel-path>: <FLAG KIND>` (grep -qF on a
# file — no pipe, no SIGPIPE). Accepted flags are counted, not printed,
# so a clean run shows zero noise and any NEW flag still surfaces.
flag() {
    # $1 = rel, $2 = kind, $3 = message
    if [ -f "$BASELINE" ] && grep -qF -- "$1: $2" "$BASELINE"; then
        accepted=$((accepted + 1))
    else
        echo "  $1: $3"
        flagged=$((flagged + 1))
    fi
}

for file in "$REG"/*/*.j2; do
    checked=$((checked + 1))
    rel="${file#"$REG"/}"
    goal="$(extract_goal "$file" || true)"
    if [ -z "$goal" ]; then
        flag "$rel" "NO GOAL" "NO GOAL |"
        continue
    fi
    if [ "${#goal}" -lt 10 ]; then
        flag "$rel" "SHORT GOAL" "SHORT GOAL | $goal"
        continue
    fi
    # Goal-wrap detection: the canonical parse (extract_goal above and
    # the template-logic audit's logic-load-goal) reads ONE {# goal: ... #}
    # block. A goal wrapped across consecutive blocks parses as only
    # its first line, which ends mid-phrase.
    case "$goal" in
        *[.!?]*) ;;
        *)
            flag "$rel" "GOAL WRAP?" "GOAL WRAP? | ${goal:0:90}"
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
            flag "$rel" "LOW OVERLAP" "LOW OVERLAP ${overlap}% | ${goal:0:90}"
        fi
    fi
done

# ── Body-side: D/P labelling presence (P8.4) ────────────────────────────
# Separate counters from the template checks above: the template baseline
# and its accepted-flags ledger govern templates only. Grep reads each
# SKILL.md directly (no pipe — grep -q's early exit is safe on a file).
# The SKILLS_DIR seam (the LEAN_SPEC_DIR pattern in check-lean-spec-pins.sh):
# overridable so check-skill-corpus-dp-labels-selftest.sh can drive the D/P
# checks over a fixture skills tree. The template checks above always run
# against the live registry (read-only, green).
SKILLS_DIR="${SKILLS_DIR:-$SCRIPT_DIR/../../../.agents/skills}"
# The oracle vocabulary that makes a SKILL.md computation-prescribing (P8.4).
# Widened 2026-10-08 with the benchmark vocabulary (cargo bench / cargo tree /
# criterion) after the D/P audit found gpui-bench escaping the presence floor
# (measured: 21 skills match the vocabulary, 20 already admitted). The bare
# `criterion` alternative can prose-match ("a criterion is...") —
# over-inclusion is adjudicated at prescreen time; under-inclusion was the
# audit's gap.
dp_oracle_pattern='lisp_eval|lean_check|`cargo`|cargo (test|check|clippy|bench|tree)|criterion|scenario_[a-z_]+\('
dp_total=0
dp_labeled=0
dp_flagged=0
for skill_md in "$SKILLS_DIR"/*/SKILL.md; do
    [ -f "$skill_md" ] || continue
    if grep -qE "$dp_oracle_pattern" "$skill_md"; then
        dp_total=$((dp_total + 1))
        if grep -qE '\*\*D/P|^#+ D/P|D/P labell?ing' "$skill_md"; then
            dp_labeled=$((dp_labeled + 1))
        else
            dp_flagged=$((dp_flagged + 1))
            echo "  ${skill_md#"$SKILLS_DIR"/}: computation-prescribing SKILL.md carries no D/P labelling section (P8.4)"
        fi
    fi
done
echo "checked $dp_total computation-prescribing SKILL.mds; $dp_labeled carry D/P labelling; $dp_flagged flagged"

# ── Body-side: label-vs-tool consistency (P8.4, check 6) ────────────────
# The conservative floor of the doctrine's "checkably wrong" example
# (PRINCIPLES.md: a section that labels a lisp_eval-calling skill "all
# P"): a SKILL.md that names a D oracle must assert a D regime somewhere
# in the file. Whole-file marker check, not section extraction — the four
# observed section forms (tables, bold paragraphs, prose seams, indented
# headers) make extraction fragile, and a false-flagging sensor gets
# ignored. Honest limitation: a file whose Instructions carry inline (D)
# markers passes even if its D/P section mislabels; per-step routing
# correctness stays P (PRINCIPLES.md), critiqued by the operator and the
# named oracle. Mention-not-invocation edges ("do NOT use lisp_eval")
# are adjudicated from the flag, not the regex. Codified 2026-10-08 from
# the D/P audit's hand-run label-vs-tool sweep (which false-positived on
# four section forms before this calibration: 0 flags on the live
# 61-skill corpus).
dp_marker_pattern='\(D\b|\*\*D\*\*|\| *D[ |+]|P/D|are D\b|is D\b|D for\b|D —|D \('
dp_coherent=0
dp_incoherent=0
for skill_md in "$SKILLS_DIR"/*/SKILL.md; do
    [ -f "$skill_md" ] || continue
    if grep -qE "$dp_oracle_pattern" "$skill_md"; then
        if grep -qE "$dp_marker_pattern" "$skill_md"; then
            dp_coherent=$((dp_coherent + 1))
        else
            dp_incoherent=$((dp_incoherent + 1))
            echo "  ${skill_md#"$SKILLS_DIR"/}: names a D oracle but asserts no D regime anywhere (P8.4 label-vs-tool floor)"
        fi
    fi
done
echo "checked $((dp_coherent + dp_incoherent)) oracle-naming SKILL.mds; $dp_coherent carry a D regime marker; $dp_incoherent flagged"

echo "checked $checked templates; $flagged flagged; $accepted accepted (baseline)"
