#!/usr/bin/env bash
# zk-ref-license-gate.sh — fail-closed license allowlist gate over a corpus census.
#
# Every source in the census must carry a license entry in the license map,
# and every license must be a member of the zk-ref-open allowlist (decision
# D10, 2026-10-05: PD, CC0/CC BY/CC BY-SA, arXiv non-exclusive, W3C/IETF,
# open-project docs, vendored ontology mirrors, operator-owned; amended
# 2026-10-07: Vatican free-use — Libreria Editrice Vaticana's published terms
# permit reproduction of the Holy Father's texts for non-commercial use with
# attribution of the source, the class covering the encyclical quotations
# embedded in the Magnifica Humanitas values charter). NC and ND
# variants are excluded by construction: they are not in the allowlist, so
# any source carrying them fails here.
#
# The gate fails closed: a missing map entry, an empty license field, or an
# unknown license class is a failure, never a pass.
#
# usage: zk-ref-license-gate.sh <license-map-tsv> <census-sha256-file>
#   license-map-tsv: "source\tlicense" per line, header row required
#   census-sha256-file: "<sha256>  <name>" per line (the shipped denominator)
# exit 0 = gate green; exit 1 = violations printed; exit 64 = usage error

set -uo pipefail

if [[ $# -ne 2 ]]; then
    echo "usage: $0 <license-map-tsv> <census-sha256-file>" >&2
    exit 64
fi
MAP="$1"
CENSUS="$2"

for f in "$MAP" "$CENSUS"; do
    if [[ ! -f "$f" ]]; then
        echo "license gate: input not found: $f" >&2
        exit 64
    fi
done

# The D10 allowlist (T3 decisions, 2026-10-05; amended 2026-10-07 — the
# operator's D10-a ruling adding the Vatican free-use class). Membership is
# exact.
declare -A ALLOWED=(
    [public_domain]=1
    [cc_open]=1
    [arxiv_nonexclusive]=1
    [w3c_ietf_open]=1
    [open_project_docs]=1
    [vendored_ontology_mirror]=1
    [operator_owned]=1
    [vatican_free_use]=1
)

# Load the license map (name -> license), rejecting malformed rows.
declare -A LICENSE
map_rows=0
while IFS=$'\t' read -r name license; do
    [[ "$name" == "source" ]] && continue
    if [[ -z "$name" || -z "$license" ]]; then
        echo "license gate: malformed map row (empty name or license): $name" >&2
        exit 1
    fi
    LICENSE["$name"]="$license"
    map_rows=$((map_rows + 1))
done < "$MAP"

census_rows=0
missing=0
not_allowed=0
declare -a violations=()

while read -r hash name; do
    [[ -z "$name" ]] && continue
    census_rows=$((census_rows + 1))
    lic="${LICENSE[$name]:-}"
    if [[ -z "$lic" ]]; then
        violations+=("MISSING-LICENSE-ENTRY $name")
        missing=$((missing + 1))
        continue
    fi
    if [[ -z "${ALLOWED[$lic]:-}" ]]; then
        violations+=("NOT-ALLOWLISTED $name ($lic)")
        not_allowed=$((not_allowed + 1))
    fi
done < "$CENSUS"

# Map entries naming files outside the census are stale — surface them too.
stale=0
for name in "${!LICENSE[@]}"; do
    if ! grep -qF "$name" "$CENSUS"; then
        violations+=("STALE-MAP-ENTRY $name")
        stale=$((stale + 1))
    fi
done

for v in "${violations[@]}"; do
    echo "license gate: $v" >&2
done

echo "license gate: census=$census_rows map=$map_rows missing=$missing not-allowed=$not_allowed stale=$stale"

if [[ "$missing" -gt 0 || "$not_allowed" -gt 0 || "$stale" -gt 0 ]]; then
    echo "license gate: FAIL (closed)" >&2
    exit 1
fi
echo "license gate: PASS — every census source carries an allowlisted license"
