#!/usr/bin/env bash
# Deterministic seam map for the kask-seam-audit skill (replaces a model step).
#
#   divergence_surface  the file column of the live rows of DIVERGENCE.md's
#                       numbered table: backticked paths, brace groups
#                       expanded, kept only where the path exists. A directory
#                       ends with "/" and covers everything under it.
#   unresolved          path-shaped tokens of that column that do not exist
#                       (removal records, deleted files).
#   prior_verdicts      each prior with verdict "live" when its artifact is an
#                       existing path or a fixed-string match in crates/ or
#                       kask/ (docs/ and target/ excluded), else "phantom".
#
# Usage: seam-map.sh <prior_rules.json>   (a JSON array of {prior, artifact, expected})
# SEAM_MAP_ROOT overrides the repository root (the test fixture seam).
set -euo pipefail

root=${SEAM_MAP_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)}
priors=${1:?usage: seam-map.sh <prior_rules.json>}
[ -f "$priors" ] || { echo "seam-map: prior rules file not found: $priors" >&2; exit 2; }
priors=$(cd "$(dirname "$priors")" && pwd)/$(basename "$priors")
register="$root/DIVERGENCE.md"
[ -f "$register" ] || { echo "seam-map: $register not found" >&2; exit 1; }
jq -e 'type == "array"' "$priors" > /dev/null || { echo "seam-map: $priors is not a JSON array" >&2; exit 2; }
cd "$root"

tokens=$(awk -F '|' '
    /^## The divergence surface/ { inside = 1; next }
    inside && /^## / { exit }
    inside && /^\| D[0-9]+[[:space:]]*\|/ { print $4 }
' "$register" | { grep -oE '`[^`]+`' || true; } | tr -d '`' | awk '
    function expand(tok,    s, e, pre, body, post, n, parts, i, item) {
        s = index(tok, "{"); e = index(tok, "}")
        if (s == 0 || e < s) { print tok; return }
        pre = substr(tok, 1, s - 1); body = substr(tok, s + 1, e - s - 1); post = substr(tok, e + 1)
        n = split(body, parts, ",")
        for (i = 1; i <= n; i++) {
            item = parts[i]; sub(/^[[:space:]]+/, "", item); sub(/[[:space:]].*$/, "", item)
            if (item != "") expand(pre item post)
        }
    }
    { expand($0) }' | sort -u)

if [ -z "$tokens" ]; then
    echo "seam-map: no rows found in the divergence surface table of $register" >&2
    exit 1
fi

surface=()
unresolved=()
while IFS= read -r tok; do
    [ -n "$tok" ] || continue
    if [[ "$tok" == */* ]]; then
        [[ "$tok" =~ ^[A-Za-z0-9_.-]+(/[A-Za-z0-9_.-]+)*/?$ ]] || continue
        if [ -d "$tok" ]; then
            surface+=("${tok%/}/")
        elif [ -f "$tok" ]; then
            surface+=("$tok")
        else
            unresolved+=("$tok")
        fi
    elif [ -f "$tok" ]; then
        surface+=("$tok")
    fi
done <<< "$tokens"

verdicts=$(jq -c '.[]' "$priors" | while IFS= read -r prior; do
    artifact=$(jq -r '.artifact // ""' <<< "$prior")
    count=0
    first=""
    if [ -n "$artifact" ]; then
        if [ -e "$artifact" ]; then
            count=1
            first="$artifact (path exists)"
        else
            hits=$(grep -rlF --exclude-dir=target --exclude-dir=docs --exclude-dir=.git -- "$artifact" crates kask 2> /dev/null | sort || true)
            count=$(grep -c . <<< "$hits" || true)
            first=$(head -n 1 <<< "$hits")
        fi
    fi
    jq -c --argjson count "$count" --arg first "$first" '
        . + (if $count > 0
             then {verdict: "live", evidence: "\($count) file(s); first: \($first)"}
             else {verdict: "phantom", evidence: "no existing path and no fixed-string match in crates/ or kask/ (docs/ and target/ excluded)"}
             end)' <<< "$prior"
done | jq -s '.')

jq -n \
    --argjson surface "$(printf '%s\n' "${surface[@]}" | jq -R 'select(length > 0)' | jq -s 'unique')" \
    --argjson unresolved "$(printf '%s\n' "${unresolved[@]}" | jq -R 'select(length > 0)' | jq -s 'unique')" \
    --argjson verdicts "$verdicts" \
    '{divergence_surface: $surface, unresolved: $unresolved, prior_verdicts: $verdicts}'
