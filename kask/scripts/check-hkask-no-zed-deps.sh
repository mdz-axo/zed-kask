#!/usr/bin/env bash
# Check the zed-kask fork's governing invariant: **no hKask crate depends on a
# zed-kask crate.** (Architecture plan §13.1 / DIVERGENCE.md.)
#
# The zed-kask↔hKask connection is one-directional: zed-kask depends on hKask
# keep-crates; hKask crates never depend on zed-kask. The sole bidirectional
# seam is the zed-kask-side bridge crate (`kask_bridge`), which lives in the
# zed-kask tree, not here. If an hKask crate grew a dependency on a zed-kask
# crate (e.g. `gpui`, `language_model`, `context_server`, `kask_bridge`), the
# dependency direction would invert and hKask would no longer compile
# standalone — a P5/P7 violation and a fork-coupling smell.
#
# Per zed-host-architecture-plan.md §13.1, the invariant applies to
# hKask crates — i.e. those under `kask/crates/hkask-*` and
# `kask/mcp-servers/hkask-*`. The bridge crate `kask_bridge` lives under
# `kask/crates/` too but is zed-kask-side (D8), NOT hKask — it is the
# documented bidirectional seam and is exempt by construction because this
# scan only visits `hkask-*` paths.
#
# This gate asks Cargo for the resolved dependency package identity; it does
# not parse dependency keys itself. That single model covers direct keys,
# renamed packages (`alias = { package = "gpui", ... }`), quoted keys,
# target-specific tables, workspace inheritance, multiline inline tables, and
# comments. A hand-written TOML parser had both false negatives (quoted keys)
# and false positives (`# package = "gpui"` in a comment).
#
# Cargo metadata also reports each path dependency's resolved absolute path.
# Any hKask dependency entering the zed-side `crates/` tree, or the explicit
# `kask_bridge` seam, is an inversion even if its package name is new and not
# yet present in the denylist. This keeps the gate future-complete for local
# zed crates while the name check covers registry/workspace dependencies whose
# path is unavailable.
#
# Enabled in CI via the §13.1 step of the `check` job in
# `.github/workflows/kask-invariants.yml`. Run locally: `scripts/check-hkask-no-zed-deps.sh`

set -euo pipefail
KASK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO_ROOT="${REPO_ROOT:-$(git -C "$KASK_ROOT" rev-parse --show-toplevel)}"
cd "$KASK_ROOT"

FAIL=0
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

command -v cargo >/dev/null 2>&1 || {
    echo "FAIL: cargo is required to resolve manifest dependency identity." >&2
    exit 1
}
command -v jq >/dev/null 2>&1 || {
    echo "FAIL: jq is required to read cargo metadata." >&2
    exit 1
}

# zed-kask-only package names. Cargo metadata reports the real package name in
# `.dependencies[].name` and the local alias (when any) in `.rename`.
ZED_CRATES='gpui|gpui_tokio|gpui_platform|gpui_macros|language_model|language_model_core|language_models|language_models_cloud|context_server|agent|agent_skills|agent_ui|agent_servers|agent_settings|acp_tools|credentials_provider|zed_credentials_provider|release_channel|paths|editor|workspace|theme|settings|ui|kask_bridge'

# hKask crates only — those under kask/crates/hkask-* and
# kask/mcp-servers/hkask-*. kask_bridge is zed-kask-side and excluded.
# MANIFEST_PATHS remains overridable for the public selftest seam.
if [ -n "${MANIFEST_PATHS+x}" ]; then
    # shellcheck disable=SC2206 # fixture API: space-separated find roots
    roots=($MANIFEST_PATHS)
else
    roots=(./crates/hkask-* ./mcp-servers/hkask-*)
fi

manifests=()
while IFS= read -r -d '' manifest; do
    manifests+=("$manifest")
done < <(find "${roots[@]}" -name Cargo.toml -not -path '*/target/*' -print0 2>/dev/null || true)

if [ "${#manifests[@]}" -eq 0 ]; then
    echo "OK: no hKask manifests matched — no inverted dependencies to report."
    exit 0
fi

# The production path resolves the workspace once. Fixture paths may each be
# independent workspaces, so they are resolved one manifest at a time below.
workspace_metadata="$TMPDIR/workspace-metadata.json"
if [ -z "${MANIFEST_PATHS+x}" ]; then
    if ! cargo metadata --manifest-path "$REPO_ROOT/Cargo.toml" \
        --format-version 1 --no-deps >"$workspace_metadata" 2>"$TMPDIR/metadata.err"; then
        echo "FAIL: cargo metadata could not resolve the workspace:" >&2
        cat "$TMPDIR/metadata.err" >&2
        exit 1
    fi
fi

echo "Checking hKask Cargo.toml resolved dependency package identities..."
for manifest in "${manifests[@]}"; do
    manifest_abs=$(realpath "$manifest")
    metadata_file="$workspace_metadata"
    if [ -n "${MANIFEST_PATHS+x}" ]; then
        metadata_file="$TMPDIR/metadata.json"
        if ! cargo metadata --manifest-path "$manifest_abs" \
            --format-version 1 --no-deps >"$metadata_file" 2>"$TMPDIR/metadata.err"; then
            echo "FAIL: cargo metadata could not resolve $manifest:" >&2
            cat "$TMPDIR/metadata.err" >&2
            FAIL=1
            continue
        fi
    fi

    if ! package_count=$(jq -r --arg manifest "$manifest_abs" \
        '[.packages[] | select(.manifest_path == $manifest)] | length' "$metadata_file"); then
        echo "FAIL: could not read package identity from cargo metadata for $manifest." >&2
        FAIL=1
        continue
    fi
    if [ "$package_count" -ne 1 ]; then
        echo "FAIL: expected exactly one package for $manifest, cargo metadata reported $package_count." >&2
        FAIL=1
        continue
    fi

    dependencies_file="$TMPDIR/dependencies.tsv"
    if ! jq -r --arg manifest "$manifest_abs" '
        .packages[]
        | select(.manifest_path == $manifest)
        | .dependencies[]
        | [.name, (.rename // "-"), (.kind // "normal"), (.target // "-"), (.path // "-")]
        | @tsv
    ' "$metadata_file" >"$dependencies_file"; then
        echo "FAIL: could not read dependencies from cargo metadata for $manifest." >&2
        FAIL=1
        continue
    fi

    while IFS=$'\t' read -r package_name local_alias dependency_kind target dependency_path; do
        [ -n "$package_name" ] || continue
        path_inversion=false
        if [ "$dependency_path" != "-" ]; then
            case "$dependency_path" in
                "$REPO_ROOT"/crates/*|"$REPO_ROOT"/kask/crates/kask_bridge)
                    path_inversion=true
                    ;;
            esac
        fi
        if [[ "$package_name" =~ ^($ZED_CRATES)$ ]] || [ "$path_inversion" = true ]; then
            alias_note=""
            if [ "$local_alias" != "-" ]; then
                alias_note=" (renamed locally as '$local_alias')"
            fi
            target_note=""
            if [ "$target" != "-" ]; then
                target_note=" for target '$target'"
            fi
            path_note=""
            if [ "$path_inversion" = true ]; then
                path_note=" at zed-side path '$dependency_path'"
            fi
            echo "VIOLATION: $manifest depends on zed-kask package '$package_name'$alias_note [$dependency_kind]$target_note$path_note" >&2
            FAIL=1
        fi
    done < "$dependencies_file"
done

if [ "$FAIL" -ne 0 ]; then
    echo "FAIL: hKask must not depend on zed-kask (plan §13.1 / DIVERGENCE.md)." >&2
    echo "      Move the logic into a zed-kask-side adapter behind an hKask port." >&2
    exit 1
fi

echo "OK: no hKask crate depends on a zed-kask crate (invariant §13.1 holds)."