#!/usr/bin/env bash
# Reject missing, duplicate or stale ledger identities and register count drift; verdicts require separate evidence review.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
if (( $# != 0 && $# != 2 )); then
    echo 'usage: check-d-seam-audit-ledger.sh [DIVERGENCE.md ledger.md]' >&2
    exit 2
fi
register=${1:-"$root/DIVERGENCE.md"}
ledger=${2:-"$root/kask/docs/research/d-seam-audit.md"}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

awk -F '|' '
    /^## The divergence surface / { inside=1; next }
    inside && /^## / { exit }
    inside && /^\| D[0-9]+[[:space:]]*\|/ {
        id=$2; gsub(/[[:space:]]/, "", id); print id
    }
' "$register" > "$tmp/live"
awk -F '|' '
    /^\| D[0-9]+[[:space:]]*\|/ {
        id=$2; gsub(/[[:space:]]/, "", id); print id
    }
' "$ledger" > "$tmp/ledger"

if [[ ! -s "$tmp/live" || ! -s "$tmp/ledger" ]]; then
    echo 'empty or unrecognized divergence table or audit ledger' >&2
    exit 1
fi
for file in live ledger; do
    if sort "$tmp/$file" | uniq -d | grep .; then
        echo "duplicate $file seam IDs" >&2
        exit 1
    fi
done
if ! diff -u <(sort "$tmp/live") <(sort "$tmp/ledger"); then
    echo 'audit ledger does not cover exactly the live D-seams' >&2
    exit 1
fi

# Counts reconciliation (2026-10-31, operator-ratified walking review): the
# identity check above reads green when both files drift together, so the
# preamble's claimed counts get their own assert against the table rows and
# the retired-seams list (observed live: register/ledger moves reported done
# from the preamble while both tables still carried the rows).
status_text=$(awk '
    /^## Status at a glance/ { inside=1; next }
    inside && /^## / { exit }
    inside { print }
' "$register")
if [[ -z "$status_text" ]]; then
    echo 'register has no Status at a glance section to reconcile' >&2
    exit 1
fi
preamble_claim() {
    # Strict single-match numeric claim extractor; exits unless exactly one
    # pattern match exists in the Status at a glance section.
    local pattern=$1 n number
    n=$({ grep -oE "$pattern" <<<"$status_text" || true; } | wc -l)
    if (( n != 1 )); then
        echo "cannot parse the register preamble counts: expected exactly one '$pattern', found $n" >&2
        exit 1
    fi
    number=$(grep -oE "$pattern" <<<"$status_text" | grep -oE '[0-9]+')
    printf '%s' "$number"
}
active_claimed=$(preamble_claim '[0-9]+ active seams')
retired_claimed=$(preamble_claim '[0-9]+ retired')
records_text=$(grep -oE 'removal records D[0-9]+.D[0-9]+' <<<"$status_text" || true)
if [[ -z "$records_text" ]]; then
    echo "cannot parse the register preamble counts: no 'removal records D<first>–D<last>' convention found" >&2
    exit 1
fi
mapfile -t records_bounds < <(grep -oE '[0-9]+' <<<"$records_text")
if (( ${#records_bounds[@]} != 2 )); then
    echo "cannot parse the removal-records range in the register preamble (expected two bounds, found ${#records_bounds[@]})" >&2
    exit 1
fi
records_claimed=$(( records_bounds[1] - records_bounds[0] + 1 ))
rows=$(wc -l < "$tmp/live")
retired_numbers=$(awk '
    /^### Retired seams/ { inside=1; next }
    inside && /^## / { exit }
    inside { print }
' "$register" | { grep -oE '\*\*D[^*]*\*\*' || true; } | { grep -oE 'D[0-9]+' || true; } | wc -l)
if (( active_claimed + records_claimed != rows )); then
    echo "register counts disagree with the table: preamble claims ${active_claimed} active + ${records_claimed} removal records = $((active_claimed + records_claimed)), the table holds ${rows} rows" >&2
    exit 1
fi
if (( retired_claimed != retired_numbers )); then
    echo "register retired count disagrees with the retired-seams list: preamble claims ${retired_claimed}, the list holds ${retired_numbers} numbers" >&2
    exit 1
fi
printf 'audit ledger: %s live seams reconciled; counts reconciled (%s active + %s removal records = %s rows; %s retired numbers) — identity + counts, not verdicts or pins\n' "$rows" "$active_claimed" "$records_claimed" "$rows" "$retired_numbers"
