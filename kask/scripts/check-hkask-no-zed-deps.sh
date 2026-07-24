#!/usr/bin/env bash
# Check the governing invariant (§13.1): **no hKask crate depends on a zed-kask
# crate.** Under the full merge (§14), hKask crates live under kask/crates/hkask-*
# and kask/mcp-servers/hkask-mcp-*. zed-kask-side crates (kask_bridge, kask_panel)
# ARE allowed to depend on zed crates — they are the adapters/panels, not hKask.
#
# This gate scans ONLY hkask-* Cargo.toml files (by name prefix) for two signals:
#   1. a dependency-section key matching a zed-only crate denylist; and
#   2. a path-dep that escapes the kask/ namespace (resolves outside kask/).
#
# Run from the zed-kask repo root: bash kask/scripts/check-hkask-no-zed-deps.sh
#
# NOTE on path resolution: a manifest at kask/mcp-servers/X/Cargo.toml may
# legitimately use path = "../../crates/Y" to reach kask/crates/Y, which is
# still inside kask/. The check resolves each path-dep against the manifest's
# directory and rejects only those that escape kask/.

set -euo pipefail
cd "$(dirname "$0")/../.." # → zed-kask repo root

FAIL=0
TMPFILE=$(mktemp)
trap 'rm -f "$TMPFILE"' EXIT

# zed-kask-only crate names. hKask uses hkask-* prefixes, so a bare dep on any
# of these names in an hkask-* Cargo.toml can only be a zed-kask crate.
ZED_CRATES='gpui|gpui_tokio|gpui_platform|gpui_macros|language_model|language_model_core|language_models|language_models_cloud|context_server|agent|agent_skills|agent_ui|agent_servers|agent_settings|acp_tools|credentials_provider|zed_credentials_provider|release_channel|paths|editor|workspace|theme|settings|ui|kask_bridge|kask_panel'

# Only hKask crates (hkask-* prefix). kask_bridge / kask_panel are zed-kask-side
# and are EXCLUDED — they are the adapters/panels, allowed to depend on zed.
hkask_manifests=$(find kask/crates kask/mcp-servers -name Cargo.toml 2>/dev/null | grep -E '/hkask-' || true)

if [ -z "$hkask_manifests" ]; then
    echo "OK: no hKask crates found under kask/ yet (migration T0.6 not done)."
    exit 0
fi

echo "Checking hKask Cargo.toml dependency sections for zed-kask crate names..."
while IFS= read -r manifest; do
    awk -v file="$manifest" -v deny="$ZED_CRATES" '
        /^\[/ { in_dep = ($0 ~ /\[[^]]*dependencies/) ? 1 : 0 }
        in_dep && !/^\[/ && $0 ~ "^(" deny ")([[:space:]]*=|[.]workspace)" {
            print file ":" FNR ":" $0
        }
    ' "$manifest" > "$TMPFILE"
    if [ -s "$TMPFILE" ]; then
        echo "VIOLATION: $manifest depends on a zed-kask crate (inverted direction):" >&2
        cat "$TMPFILE" >&2
        FAIL=1
    fi
done <<< "$hkask_manifests"

echo "Checking hKask Cargo.toml for path-deps that escape kask/..."
while IFS= read -r manifest; do
    # For each path-dep, resolve it against the manifest's directory and
    # reject only if the resolved real path escapes the kask/ namespace.
    manifest_dir=$(dirname "$manifest")
    while IFS= read -r dep_path; do
        # Strip surrounding quotes from the captured path value.
        dep_path=${dep_path#\"}
        dep_path=${dep_path%\"}
        resolved=$(realpath --relative-to="$PWD/kask" "$manifest_dir/$dep_path" 2>/dev/null || true)
        # realpath --relative-to prints "../..." when the target is outside kask/.
        # An empty result means the target doesn't exist yet (e.g. a not-yet-
# migrated crate) — flag it so we notice, but don't hard-fail on missing
# paths alone; the denylist check above catches zed-name deps.
        if [ -z "$resolved" ]; then
            echo "WARN: $manifest path-dep '$dep_path' did not resolve (target missing?) — not a §13.1 violation by itself."
            continue
        fi
        case "$resolved" in
            ../*)
                echo "VIOLATION: $manifest has a path-dep escaping kask/ ($dep_path → $resolved):" >&2
                echo "  $dep_path" >&2
                FAIL=1
                ;;
        esac
    done < <(grep -Eo 'path[[:space:]]*=[[:space:]]*"[^"]*"' "$manifest" | sed -E 's/.*=[[:space:]]*//')
done <<< "$hkask_manifests"

if [ "$FAIL" -ne 0 ]; then
    echo "FAIL: hKask crates must not depend on zed-kask (plan §13.1 / DIVERGENCE.md)." >&2
    echo "      Move the logic into kask_bridge (the sole bidirectional seam)." >&2
    exit 1
fi

echo "OK: no hKask crate depends on a zed-kask crate (invariant §13.1 holds)."
