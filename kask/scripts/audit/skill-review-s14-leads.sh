#!/usr/bin/env bash
# Read-only S14 lead generator: flag template contract outputs that look like
# a derived quantity over the template's OWN input arrays (template-computable
# via a Jinja filter, or lisp_eval-computable) rather than a judgment.
#
# A lead, not a verdict: an output named `finding_count` over an input
# `findings: array` is the canonical violation; an output `verdict` over the
# same input is a judgment and stays.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
TEMPLATES="$ROOT/kask/registry/templates"

find "$TEMPLATES" -name '*.j2' | sort | while read -r f; do
    # Extract the [inference] contract block (between "[inference]" and "---").
    block=$(awk '/^\[inference\]/{p=1} p&&/^---$/{exit} p' "$f")
    [ -n "$block" ] || continue

    inputs=$(printf '%s\n' "$block" | awk '/^  input:/{p=1;next} /^  output:/{p=0} p&&/^    [a-z_]+:/{print}')
    outputs=$(printf '%s\n' "$block" | awk '/^  output:/{p=1;next} p&&/^    [a-z_]+:/{print}')

    # Input field names declared as arrays.
    array_inputs=$(printf '%s\n' "$inputs" | awk -F: '/array|list/{gsub(/^ +| +$/,"",$1); print $1}')
    [ -n "$array_inputs" ] || continue

    # Output fields whose name reads as a count/aggregate.
    printf '%s\n' "$outputs" | awk -F: '{
        name=$1; gsub(/^ +| +$/,"",name);
        if (name ~ /(_count|_total|_sum|_length|_number|_proportion|_rate|_pct|_percent|_ratio|_score)$|^(count|total|sum|length|number)$/)
            print name
    }' | while read -r out; do
        rel="${f#"$TEMPLATES"/}"
        printf '%s\toutput=%s\tarray_inputs=%s\n' "$rel" "$out" "$(printf '%s' "$array_inputs" | paste -sd, -)"
    done
done
