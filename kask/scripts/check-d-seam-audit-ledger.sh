#!/usr/bin/env bash
# Reject missing, duplicate or stale ledger identities; verdicts require separate evidence review.
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
printf 'audit ledger: %s live seams reconciled (identity only; not verdicts or pins)\n' "$(wc -l < "$tmp/live")"
