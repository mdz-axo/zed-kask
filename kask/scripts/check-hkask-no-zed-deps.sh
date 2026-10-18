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
# This gate detects three inversion signals in hKask Cargo.toml files:
#   1. a `path = "..."` dependency that points into the zed-kask tree;
#   2. a dependency (in a `[dependencies]`/`[dev-dependencies]`/`[build-dependencies]`
#      section) whose key is a zed-kask-only crate (denylist below); and
#   3. a dependency RENAMED via `package = "<zed crate>"`, where the key is
#      arbitrary but the resolved package is a zed-kask crate.
# Signal 3 exists because signals 1-2 match the dependency *key*, and Cargo's
# rename syntax (`render_core = { package = "gpui", path = ... }`) produces
# exactly the same inverted dependency under a key the denylist cannot see.
# A path-dep rename is caught by signal 1, but a workspace or registry rename
# (`render_core = { package = "gpui", workspace = true }`) is not — signal 3
# is what closes that. Pinned by check-hkask-no-zed-deps-selftest.sh.
# hKask crates are `hkask-*` prefixed, so a bare dep on `gpui`/`editor`/`ui`/
# `workspace`/`settings` etc. can only be a zed-kask crate (never an hKask one).
# Only dependency *sections* are scanned (via awk), so `[package] workspace = true`
# and other non-dependency fields do not false-positive.
#
# Enabled in CI via the §13.1 step of the `check` job in
# `.github/workflows/kask-invariants.yml`. Run locally: `scripts/check-hkask-no-zed-deps.sh`

set -euo pipefail
cd "$(dirname "$0")/.."

FAIL=0
TMPFILE=$(mktemp)
trap 'rm -f "$TMPFILE"' EXIT

# zed-kask-only crate names. hKask uses `hkask-` prefixes, so none of these
# names can be a legitimate hKask-internal or crates.io dependency here.
ZED_CRATES='gpui|gpui_tokio|gpui_platform|gpui_macros|language_model|language_model_core|language_models|language_models_cloud|context_server|agent|agent_skills|agent_ui|agent_servers|agent_settings|acp_tools|credentials_provider|zed_credentials_provider|release_channel|paths|editor|workspace|theme|settings|ui|kask_bridge'

# hKask crates only — those under kask/crates/hkask-* and kask/mcp-servers/hkask-*.
# The bridge (kask_bridge) lives under kask/crates/ but is zed-kask-side
# (D8), not hKask — scanning it would false-positive on the very
# bidirectional seam §13.1 exempts (zed-host-architecture-plan.md §13.1).
# Overridable via env var so the self-test can point at a temp tree; the
# default preserves the production behavior exactly.
if [ -n "${MANIFEST_PATHS+x}" ]; then
  manifests=$(find $MANIFEST_PATHS -name Cargo.toml -not -path './target/*' 2>/dev/null || true)
else
  manifests=$(find ./crates/hkask-* ./mcp-servers/hkask-* -name Cargo.toml -not -path './target/*' 2>/dev/null || true)
fi

# 1. Any path-dep into the zed-kask tree (e.g. path = "../../Clones/zed-kask/...").
echo "Checking hKask Cargo.toml for zed-kask path-deps..."
while IFS= read -r manifest; do
    if grep -Eq 'path[[:space:]]*=[[:space:]]*"[^"]*zed-kask' "$manifest"; then
        echo "VIOLATION: $manifest references zed-kask via a path-dep:" >&2
        grep -En 'path[[:space:]]*=[[:space:]]*"[^"]*zed-kask' "$manifest" >&2
        FAIL=1
    fi
done <<< "$manifests"

# 2+3. Dependency-section signals, in one pass over each manifest.
#
#    TOML spells a dependency two ways, and the two put denylisted names in
#    different positions:
#      [dependencies]            -> keys ARE dependency names
#      [dependencies.render_core]-> keys are dependency FIELDS (version, path,
#                                   workspace, package, features)
#    Conflating them is a live defect: inside `[dependencies.serde]`, the
#    field line `workspace = true` matches the denylisted crate name
#    `workspace` and reports a legitimate manifest as a violation. No hKask
#    manifest uses the subtable form today, so this was latent — it would have
#    fired on the first one. The header classification below types each
#    section before matching, so a field name is never read as a crate name.
#
#    Emitted reasons:
#      KEY    — a dependency whose name is a zed-kask crate (either as a key
#               in a bare dependency table, or as a `[dependencies.<name>]`
#               subtable name).
#      RENAME — a dependency renamed via `package = "<zed crate>"`, where the
#               key is arbitrary but the resolved package is zed-kask. Covers
#               both the inline-table and subtable spellings.
echo "Checking hKask Cargo.toml dependency sections for zed-kask crate names (direct and renamed)..."
while IFS= read -r manifest; do
    awk -v file="$manifest" -v deny="$ZED_CRATES" '
        /^[[:space:]]*\[/ {
            hdr = $0
            sub(/^[[:space:]]*\[/, "", hdr)
            sub(/\].*$/, "", hdr)
            in_table = 0; in_sub = 0; sub_name = ""
            # Bare dependency table, incl. [target.<cfg>.dependencies].
            if (hdr ~ /(^|\.)(dependencies|dev-dependencies|build-dependencies)$/) {
                in_table = 1
            # Per-dependency subtable: [dependencies.<name>].
            } else if (hdr ~ /(^|\.)(dependencies|dev-dependencies|build-dependencies)\.[A-Za-z0-9_-]+$/) {
                in_sub = 1
                sub_name = hdr
                sub(/^.*\./, "", sub_name)
                if (sub_name ~ "^(" deny ")$") {
                    print "KEY|" file ":" FNR ":" $0
                }
            }
            next
        }
        # In a bare table the key is the dependency name.
        in_table && $0 ~ "^(" deny ")([[:space:]]*=|[.]workspace)" {
            print "KEY|" file ":" FNR ":" $0
        }
        # In either spelling, `package = "<zed crate>"` resolves to a zed crate.
        (in_table || in_sub) && $0 ~ "package[[:space:]]*=[[:space:]]*\"(" deny ")\"" {
            print "RENAME|" file ":" FNR ":" $0
        }
    ' "$manifest" > "$TMPFILE"
    if [ -s "$TMPFILE" ]; then
        if grep -q '^KEY|' "$TMPFILE"; then
            echo "VIOLATION: $manifest depends on a zed-kask crate (inverted direction):" >&2
            sed -n 's/^KEY|//p' "$TMPFILE" >&2
        fi
        if grep -q '^RENAME|' "$TMPFILE"; then
            echo "VIOLATION: $manifest renames a zed-kask crate via package = (inverted direction):" >&2
            sed -n 's/^RENAME|//p' "$TMPFILE" >&2
        fi
        FAIL=1
    fi
done <<< "$manifests"

if [ "$FAIL" -ne 0 ]; then
    echo "FAIL: hKask must not depend on zed-kask (plan §13.1 / DIVERGENCE.md)." >&2
    echo "      Move the logic into a zed-kask-side adapter behind an hKask port." >&2
    exit 1
fi

echo "OK: no hKask crate depends on a zed-kask crate (invariant §13.1 holds)."