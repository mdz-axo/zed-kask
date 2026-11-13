#!/usr/bin/env bash
# Exercise kask/scripts/audit/seam-map.sh on a fixture tree: brace groups
# expand, prose inside a group is dropped, a directory covers its subtree,
# rows outside the numbered table are ignored, a missing path is unresolved,
# and a prior is live only on a path or a code match (a docs mention is not).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/crates/a/src" "$fixture/crates/dir/sub" "$fixture/kask/docs" "$fixture/kask/crates/k/src"
printf 'fn live_symbol() {}\n' > "$fixture/crates/a/src/x.rs"
printf '// y\n' > "$fixture/crates/a/src/y.rs"
printf '// z\n' > "$fixture/crates/dir/sub/z.rs"
printf 'docs mention docs_only_symbol\n' > "$fixture/kask/docs/note.txt"
printf '// k\n' > "$fixture/kask/crates/k/src/k.rs"
printf 'root file\n' > "$fixture/.rules"
cat > "$fixture/DIVERGENCE.md" <<'DOC'
# fixture

## The divergence surface (D1–D4)

| D | Surface | file | notes |
| --- | --- | --- | --- |
| D1 | one | `crates/a/src/{x.rs, y.rs (a note)}` + `Symbol::name` | n |
| D2 | two | `crates/dir/` + `missing/file.rs` | n |
| D3 | three | `.rules` + `not_a_file` | n |

### Retired seams

- **D9** retired `crates/retired/gone.rs`

## Other files

| D4 | ignored | `kask/crates/k/src/k.rs` | outside the table section |
DOC
printf '%s\n' '[{"prior":"p-live","artifact":"live_symbol","expected":"live"},{"prior":"p-path","artifact":"crates/dir","expected":"live"},{"prior":"p-phantom","artifact":"no_such_symbol","expected":"live"},{"prior":"p-docs","artifact":"docs_only_symbol","expected":"live"}]' > "$fixture/priors.json"

out="$(SEAM_MAP_ROOT="$fixture" bash "$repo_root/kask/scripts/audit/seam-map.sh" "$fixture/priors.json")"
expect() {
    if ! jq -e "$1" <<< "$out" > /dev/null; then
        echo "FAIL: $2" >&2
        echo "$out" >&2
        exit 1
    fi
}
expect '.divergence_surface == [".rules", "crates/a/src/x.rs", "crates/a/src/y.rs", "crates/dir/"]' 'surface: brace group expanded, prose dropped, directory slash-terminated, later section ignored'
expect '.unresolved == ["missing/file.rs"]' 'a missing path-shaped token is unresolved; the retired list is not read'
expect '[.prior_verdicts[] | .verdict] == ["live", "live", "phantom", "phantom"]' 'verdicts: symbol match, existing path, no match, docs-only mention'
expect '.prior_verdicts[0].evidence | test("^1 file\\(s\\); first: crates/a/src/x.rs$")' 'live evidence names the count and the first file'

printf '# empty\n' > "$fixture/DIVERGENCE.md"
if SEAM_MAP_ROOT="$fixture" bash "$repo_root/kask/scripts/audit/seam-map.sh" "$fixture/priors.json" > /dev/null 2>&1; then
    echo 'FAIL: a register with no table rows must be an error, not an empty surface' >&2
    exit 1
fi
echo 'OK: seam-map expands the table into an existing-path surface and settles priors by path or code match.'
