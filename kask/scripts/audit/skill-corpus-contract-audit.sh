#!/usr/bin/env bash
# Skill-corpus contract audit — the second mechanical layer of the
# skill-logic-audit corpus pass (the prescreen is the first: goal shape).
#
# For every shipped .j2 template under kask/registry/templates/ that carries
# an [inference] contract, checks contract↔body agreement:
#   1. UNUSED INPUT — a declared input the body never consumes
#      ({{ field }}, {% if field %}, {% for x in field %}).
#   2. ABSENT OUTPUT — a declared output the body never mentions
#      (the contract line is the only occurrence in the file).
#   3. UNDECLARED CONSUMPTION — a context variable the body consumes
#      that the contract does not declare.
#
# False-positive classes excluded by construction (verified against the
# pass-5 corpus audit): {% raw %} spans are stripped before extraction
# (their content is literal text, e.g. mermaid hexagon syntax); loop
# locals, {% set %}-captured names, {% macro %} names and arguments, and
# the Jinja `loop` variable are excluded from undeclared consumption;
# contract fields are read at the 4-space indent only, so nested-schema
# contracts (type/description sub-keys) do not surface as fields.
# Known under-detection (advisory, documented): only the first term of a
# multi-term {% if a and b %} condition is extracted.
#
# Templates without a contract (server-rendered shadows, pure render
# templates) are skipped and counted, not flagged.
#
# Advisory instrument: findings are proposals for read-triage, not edits.
# Usage: ./skill-corpus-contract-audit.sh

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REG="$SCRIPT_DIR/../../registry/templates"

# Strip {% raw %}...{% endraw %} spans: their content is literal text.
strip_raw() {
    awk '
        {
            line = $0; out = ""
            while (length(line) > 0) {
                if (!inraw) {
                    p = index(line, "{% raw %}")
                    if (p > 0) { out = out substr(line, 1, p - 1); line = substr(line, p + 9); inraw = 1 }
                    else { out = out line; line = "" }
                } else {
                    q = index(line, "{% endraw %}")
                    if (q > 0) { line = substr(line, q + 12); inraw = 0 }
                    else { line = "" }
                }
            }
            print out
        }
    ' "$1"
}

# Contract fields at the 4-space indent for one section (input|output).
# Field names may contain digits (step_1_result); 6+ space lines are
# nested-schema sub-keys (type/description/enum) and do not end the
# section; any shallower line (output:, visibility:, ---) does.
contract_fields() {
    awk -v sect="  $2:" '
        $0 == sect { insec = 1; next }
        insec && /^    [a-z_][a-z0-9_]*:/ { f = $1; sub(/:$/, "", f); print f; next }
        insec && /^      / { next }
        insec { insec = 0 }
    ' "$1"
}

# The template body: everything after the first lone --- (the contract
# terminator). Consumption checks run against this, so contract lines
# never count as body usage.
body_after_contract() {
    awk 'seen { print } /^---$/ { seen = 1 }' "$1"
}

checked=0
flagged=0
no_contract=0
body_file="$(mktemp)"
trap 'rm -f "$body_file"' EXIT
for file in "$REG"/*/*.j2; do
    checked=$((checked + 1))
    rel="${file#"$REG"/}"

    if ! grep -q '^  input:' "$file"; then
        no_contract=$((no_contract + 1))
        continue
    fi

    stripped="$(strip_raw "$file")"
    # The body goes to a file, not a pipe: grep -q exits at the first
    # match, and an early-exiting reader on a pipe SIGPIPEs the writer —
    # pipefail then turns a successful match into a false flag (observed
    # at ~35% per check on large bodies). On a file, -q's early exit is
    # harmless.
    body_after_contract "$file" | strip_raw - > "$body_file"

    ins="$(contract_fields "$file" input | tr '\n' ' ')"
    outs="$(contract_fields "$file" output | tr '\n' ' ')"

    # Consumed context roots: {{ var }}, {% if var %} / {% elif var %}
    # (optional `not`), and the iterable of {% for x in var %}.
    consumed="$(
        {
            printf '%s\n' "$stripped" | grep -oE '\{\{[[:space:]]*[a-zA-Z_][a-zA-Z0-9_]*' | sed 's/{{ *//' || true
            printf '%s\n' "$stripped" | grep -oE '\{%-?[[:space:]]*(if|elif)[[:space:]]+(not[[:space:]]+)?[a-zA-Z_][a-zA-Z0-9_]*' | sed 's/.*[[:space:]]//' || true
            printf '%s\n' "$stripped" | grep -oE '\{%-?[[:space:]]*for[[:space:]]+[a-zA-Z_][a-zA-Z0-9_]*[[:space:]]+in[[:space:]]+[a-zA-Z_][a-zA-Z0-9_]*' | awk '{print $NF}' || true
        } | sort -u | tr '\n' ' '
    )"

    # Locals: for-loop variables, set-captured names, macro names and
    # arguments — defined inside the template, never context inputs.
    locals="$(
        {
            printf '%s\n' "$stripped" | grep -oE '\{%-?[[:space:]]*for[[:space:]]+[a-zA-Z_][a-zA-Z0-9_]*' | awk '{print $3}' || true
            printf '%s\n' "$stripped" | grep -oE '\{%-?[[:space:]]*set[[:space:]]+[a-zA-Z_][a-zA-Z0-9_]*' | awk '{print $3}' || true
            printf '%s\n' "$stripped" | grep -oE '\{%-?[[:space:]]*macro[[:space:]]+[a-zA-Z_][a-zA-Z0-9_]*\([^)]*\)' \
                | sed -E 's/.*macro[[:space:]]+([a-zA-Z_][a-zA-Z0-9_]*)\(([^)]*)\).*/\1 \2/' \
                | tr ',' ' ' | tr -s ' ' '\n' || true
        } | sort -u | tr '\n' ' '
    )"

    found=0

    # 1. Unused inputs: the field never appears as a word in the body
    #    (any consumption form — {{ }}, {% if %}, ~ concat — counts).
    for field in $ins; do
        if ! grep -qw -- "$field" "$body_file"; then
            echo "  $rel: unused-input: $field"
            found=1
        fi
    done

    # 2. Absent outputs (contract line is the only mention).
    for field in $outs; do
        n=$(grep -wc -- "$field" "$file" || true)
        if [ "$n" -le 1 ]; then
            echo "  $rel: absent-output: $field"
            found=1
        fi
    done

    # 3. Undeclared consumption.
    for var in $consumed; do
        case " $ins $locals loop " in
            *" $var "*) ;;
            *)
                echo "  $rel: undeclared: $var"
                found=1
                ;;
        esac
    done

    if [ "$found" -eq 1 ]; then
        flagged=$((flagged + 1))
    fi
done
echo "checked $checked templates; $flagged flagged; $no_contract skipped (no contract)"
