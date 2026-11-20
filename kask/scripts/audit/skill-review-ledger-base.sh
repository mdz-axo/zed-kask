#!/usr/bin/env bash
# Read-only: emit the mechanical base of the skill-review ledger.
# Columns: skill, templates, gates, tpls_referenced_missing, composition_edges,
#          onto_anchor_derived_cites, doc_cites, loop_init, loop_target,
#          loop_check, loop_bound, loop_act
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
SKILLS="$ROOT/.agents/skills"
TEMPLATES="$ROOT/kask/registry/templates"

printf 'skill\ttemplates\tgates\tmissing_tpl_refs\tcomp_edges\tderived_cites\tdoc_cites\tinit\ttarget\tcheck\tbound\tact\n'
for d in "$SKILLS"/*/; do
    n="$(basename "$d")"; md="$d/SKILL.md"; [ -f "$md" ] || continue
    tpl=$(ls "$TEMPLATES/$n"/*.j2 2>/dev/null | wc -l)
    gates=$(grep -o -E '`[A-Z][A-Z0-9]*-[0-9A-Z]+`' "$md" | tr -d '`' | sort -u | paste -sd, -)
    missing=0
    for t in $(grep -o -E '[a-z][a-z0-9-]*/[a-z0-9-]+\.j2' "$md" | sort -u); do
        [ -f "$TEMPLATES/$t" ] || missing=$((missing+1))
    done
    edges=$(grep -o -E '`[a-z][a-z0-9]*(-[a-z0-9]+)+`' "$md" | tr -d '`' | sort -u \
            | while read -r t; do [ -d "$SKILLS/$t" ] && [ "$t" != "$n" ] && echo "$t"; done | wc -l)
    dcites=$(grep -o -E 'onto_anchor`? *(→|->) *derived `[a-z0-9_]+`' "$md" | wc -l)
    doccites=$(grep -o -E 'kask/[a-zA-Z0-9_./-]+\.md' "$md" | sort -u | wc -l)
    init=$(grep -qi 'initial condition' "$md" && echo y || echo n)
    tgt=$(grep -qi 'target condition' "$md" && echo y || echo n)
    chk=$(grep -qE '`[A-Z][A-Z0-9]*-[0-9A-Z]+`|^Check:|lisp_eval' "$md" && echo y || echo n)
    bnd=$(grep -qiE 'bound:|max[_ ](rounds|retries|attempts)|at most|one (re-|re-?trace|retry|correction|pass)|escalate after|after .* failures|three ' "$md" && echo y || echo n)
    act=$(grep -qiE 're-enter|reenter|escalate|halt|stop|terminal' "$md" && echo y || echo n)
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$n" "$tpl" "$gates" "$missing" "$edges" "$dcites" "$doccites" "$init" "$tgt" "$chk" "$bnd" "$act"
done
