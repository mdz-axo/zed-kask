#!/usr/bin/env bash
# Read-only preflight helper for zed-kask upstream merges (`upstream-rebase` skill).
#
# Computes:
#   1. Commit/file delta counts between <merge-base> and <upstream-ref> vs HEAD.
#   2. Non-deleted both-changed crate names (for `./script/clippy -p ...`).
#   3. Step 0 DIVERGENCE.md TOUCHED vs UNTOUCHED live D-row classification.
#   4. With `--ur1`: JSON payload of {file, fork, upstream, markers, sites} for
#      currently unmerged (`UU`) Rust files, ready for `lisp_eval` UR-1.
#
# Usage:
#   bash kask/scripts/upstream-rebase-preflight.sh [<merge-base> <upstream-ref>]
#   bash kask/scripts/upstream-rebase-preflight.sh --ur1 [<upstream-ref>]
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

if [[ "${1:-}" == "--ur1" ]]; then
    upstream_ref="${2:-upstream/main}"
    printf '{"files":['
    first=1
    while IFS= read -r line; do
        st="${line%% *}"
        path="${line#* }"
        [[ "$st" != "UU" ]] && continue
        [[ "$path" != *.rs ]] && continue
        fork=$(git show "HEAD:$path" | wc -l)
        upstream=$(git show "$upstream_ref:$path" | wc -l)
        sites=$(git show "HEAD:$path" | grep -c "kask_bridge::\|hkask_\|hkask-" || true)
        markers=$(git show "HEAD:$path" | grep -c "// zed-kask:" || true)
        if [[ "$first" -eq 0 ]]; then
            printf ','
        fi
        first=0
        printf '{"file":"%s","fork":%d,"upstream":%d,"markers":%d,"sites":%d}' \
            "$path" "$fork" "$upstream" "$markers" "$sites"
    done < <(git --no-optional-locks status --short | grep -E '^UU ')
    printf ']}\n'
    exit 0
fi

upstream_ref="${2:-upstream/main}"
base="${1:-$(git merge-base HEAD "$upstream_ref")}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

git diff --name-only "$base" "$upstream_ref" | LC_ALL=C sort > "$tmp/upstream_changed.txt"
git diff --name-only "$base" HEAD | LC_ALL=C sort > "$tmp/fork_changed.txt"
LC_ALL=C comm -12 "$tmp/upstream_changed.txt" "$tmp/fork_changed.txt" > "$tmp/both_changed.txt"
LC_ALL=C comm -23 "$tmp/upstream_changed.txt" "$tmp/fork_changed.txt" > "$tmp/upstream_only.txt"

echo "=== Baseline ==="
echo "merge_base=$base"
echo "upstream_ref=$upstream_ref ($(git rev-parse "$upstream_ref"))"
echo "head=$(git rev-parse HEAD)"
echo "commits=$(git rev-list --count "$base..$upstream_ref")"
echo "upstream_changed=$(wc -l < "$tmp/upstream_changed.txt")"
echo "fork_changed=$(wc -l < "$tmp/fork_changed.txt")"
echo "both_changed=$(wc -l < "$tmp/both_changed.txt")"
echo "upstream_only=$(wc -l < "$tmp/upstream_only.txt")"

echo "=== Both-changed crates (existing in HEAD, for ./script/clippy) ==="
while IFS= read -r f; do
    [[ "$f" == crates/*/* ]] || continue
    crate_dir="${f#crates/}"
    crate_dir="crates/${crate_dir%%/*}"
    if [[ -f "$crate_dir/Cargo.toml" ]]; then
        awk -F'"' '/^name[[:space:]]*=[[:space:]]*"/ { print $2; exit }' "$crate_dir/Cargo.toml"
    fi
done < "$tmp/both_changed.txt" | LC_ALL=C sort -u | tee "$tmp/both_crates.txt"

echo "=== Step 0 live D-rows (DIVERGENCE.md) ==="
awk -F'|' '/^\| D[0-9]+ \|/ {
    gsub(/^ +| +$/, "", $2)
    gsub(/^ +| +$/, "", $3)
    gsub(/^ +| +$/, "", $4)
    print $2 "\t" $3 "\t" $4
}' DIVERGENCE.md > "$tmp/d_rows.tsv"

touched=0
untouched=0
while IFS=$'\t' read -r dnum _title files; do
    matched=""
    tokens=$(printf '%s\n' "$files" | awk '{
        s=$0
        while (match(s, /`[^`]+`/)) {
            print substr(s, RSTART+1, RLENGTH-2)
            s = substr(s, RSTART+RLENGTH)
        }
    }')
    for tok in $tokens; do
        if [[ "$tok" == *"{"*"}"* && "$tok" =~ ^[a-zA-Z0-9_./\{\},*-]+$ ]]; then
            expanded=$(eval printf '%s\\n' "$tok" 2>/dev/null || printf '%s\n' "$tok")
        else
            expanded="$tok"
        fi
        while IFS= read -r p; do
            [[ -z "$p" ]] && continue
            if [[ "$p" == */ ]]; then
                hits=$(grep -F "$p" "$tmp/upstream_changed.txt" || true)
            else
                hits=$(grep -Fx "$p" "$tmp/upstream_changed.txt" || true)
            fi
            if [[ -n "$hits" ]]; then
                matched+="$hits"$'\n'
            fi
        done <<< "$expanded"
    done
    if [[ -n "$matched" ]]; then
        touched=$((touched + 1))
        uniq=$(printf '%s' "$matched" | LC_ALL=C sort -u | tr '\n' ',' | sed 's/,$//')
        printf 'TOUCHED\t%s\t%s\n' "$dnum" "$uniq"
    else
        untouched=$((untouched + 1))
        printf 'UNTOUCHED\t%s\t-\n' "$dnum"
    fi
done < "$tmp/d_rows.tsv"

echo "=== Step 0 counts ==="
echo "total_live=$((touched + untouched)) touched=$touched untouched=$untouched"
