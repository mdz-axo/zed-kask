#!/usr/bin/env bash
# verify-published-bundle.sh — assert the published corpus release matches the
# committed bundle manifest, and that no stale part lingers.
#
# The installer downloads assets by NAME from
# releases/download/corpus-<id>/<name> and verifies each against the sha256
# pinned in kask/registry/zk-ref-open/bundle.json. A stale part left on the
# release is invisible to that loop (it iterates the manifest, not the
# release) but is still served to anyone who asks for it by name — so it is
# checked here explicitly.
set -euo pipefail

repo=${1:-mdz-axo/zed-kask}
tag=${2:-corpus-zk-ref-open}
manifest=${3:-kask/registry/zk-ref-open/bundle.json}

if [[ ! -f "$manifest" ]]; then
    echo "manifest not found: $manifest" >&2
    exit 66
fi

published=$(mktemp)
trap 'rm -f "$published"' EXIT
gh release view "$tag" --repo "$repo" --json assets \
    -q '.assets[] | "\(.name)\t\(.size)\t\(.digest)"' > "$published"

ok=0
bad=0
while IFS=$'\t' read -r name want; do
    line=$(grep -P "^$(printf '%s' "$name" | sed 's/[.[\*^$]/\\&/g')\t" "$published" || true)
    if [[ -z "$line" ]]; then
        echo "  MISSING  $name (not published)" >&2
        bad=$(( bad + 1 ))
        continue
    fi
    have=$(printf '%s' "$line" | cut -f3 | sed 's/^sha256://')
    if [[ "$want" == "$have" ]]; then
        echo "  OK       $name"
        ok=$(( ok + 1 ))
    else
        echo "  MISMATCH $name (manifest=$want published=$have)" >&2
        bad=$(( bad + 1 ))
    fi
done < <(jq -r '.assets[] | "\(.name)\t\(.sha256)"' "$manifest")

# Stale assets: published names the manifest does not pin. `bundle.json` is
# exempt by construction — a manifest cannot pin its own hash, so the release
# carries it as a convenience copy for humans reading the release page; the
# git-committed copy is the trust root.
stale=0
while IFS=$'\t' read -r name size digest; do
    [[ -z "$name" ]] && continue
    [[ "$name" == "bundle.json" ]] && continue
    if ! jq -e --arg n "$name" '.assets[] | select(.name == $n)' "$manifest" >/dev/null; then
        echo "  STALE    $name ($size bytes, digest=${digest:-none}) — published but not in the manifest" >&2
        stale=$(( stale + 1 ))
    fi
done < "$published"

echo "verify-published-bundle: matched=$ok mismatched=$bad stale=$stale"
if (( bad > 0 || stale > 0 )); then
    echo "verify-published-bundle: FAIL" >&2
    exit 1
fi
echo "verify-published-bundle: PASS — the release serves exactly the committed manifest"
