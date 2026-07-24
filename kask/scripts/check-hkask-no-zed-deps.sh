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
# This gate detects two inversion signals in hKask Cargo.toml files:
#   1. a `path = "..."` dependency that points into the zed-kask tree; and
#   2. a dependency (in a `[dependencies]`/`[dev-dependencies]`/`[build-dependencies]`
#      section) whose key is a zed-kask-only crate (denylist below).
# hKask crates are `hkask-*` prefixed, so a bare dep on `gpui`/`editor`/`ui`/
# `workspace`/`settings` etc. can only be a zed-kask crate (never an hKask one).
# Only dependency *sections* are scanned (via awk), so `[package] workspace = true`
# and other non-dependency fields do not false-positive.
#
# Enabled in CI via `.github/workflows/ci.yml` invariants job.
# Run locally: `scripts/check-hkask-no-zed-deps.sh`

set -euo pipefail
cd "$(dirname "$0")/.."

FAIL=0
TMPFILE=$(mktemp)
trap 'rm -f "$TMPFILE"' EXIT

# zed-kask-only crate names. hKask uses `hkask-` prefixes, so none of these
# names can be a legitimate hKask-internal or crates.io dependency here.
ZED_CRATES='gpui|gpui_tokio|gpui_platform|gpui_macros|language_model|language_model_core|language_models|language_models_cloud|context_server|agent|agent_skills|agent_ui|agent_servers|agent_settings|acp_tools|credentials_provider|zed_credentials_provider|release_channel|paths|editor|workspace|theme|settings|ui|kask_bridge|kask_panel'

# All Cargo.toml in the hKask repo (root + crates + mcp-servers), excluding build output.
manifests=$(find . -name Cargo.toml -not -path './target/*' 2>/dev/null)

# 1. Any path-dep into the zed-kask tree (e.g. path = "../../Clones/zed-kask/...").
echo "Checking hKask Cargo.toml for zed-kask path-deps..."
while IFS= read -r manifest; do
    if grep -Eq 'path[[:space:]]*=[[:space:]]*"[^"]*zed-kask' "$manifest"; then
        echo "VIOLATION: $manifest references zed-kask via a path-dep:" >&2
        grep -En 'path[[:space:]]*=[[:space:]]*"[^"]*zed-kask' "$manifest" >&2
        FAIL=1
    fi
done <<< "$manifests"

# 2. Any dependency whose key is a zed-kask-only crate name, within dependency
#    sections only. awk tracks section membership (`[dependencies]` /
#    `[dev-dependencies]` / `[build-dependencies]` / `[target.'...'.dependencies]`)
#    and matches a denylist key at the start of a dependency line. `[.]` is a
#    literal dot (no escaping). Emits `file:FNR:line` only on a match.
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
done <<< "$manifests"

if [ "$FAIL" -ne 0 ]; then
    echo "FAIL: hKask must not depend on zed-kask (plan §13.1 / DIVERGENCE.md)." >&2
    echo "      Move the logic into a zed-kask-side adapter behind an hKask port." >&2
    exit 1
fi

echo "OK: no hKask crate depends on a zed-kask crate (invariant §13.1 holds)."