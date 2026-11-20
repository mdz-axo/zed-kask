#!/usr/bin/env bash
# Check for unused crate dependencies in kask crates using the nightly
# `unused_crate_dependencies` lint. `cargo machete` finds unused Cargo.toml
# entries (deps declared but never imported in any target), while this
# script catches deps declared but never imported in the lib target specifically.
# Run both: `cargo machete` + `bash kask/scripts/check-unused-deps.sh`.
#
# This script catches the class that `cargo machete` misses: a dep in
# `[dependencies]` that the lib target never `use`s (e.g. `tokio` declared
# for the bin's `#[tokio::main]` but not used in the lib). Lib roots with
# a legitimate bin-needs-dep case carry `#![allow(unused_crate_dependencies)]`
# with an explanatory comment.
#
# Usage: bash scripts/check-unused-deps.sh
set -euo pipefail
cd "$(dirname "$0")/.."

echo "=== Unused crate dependencies (kask lib crates) ==="

command -v cargo >/dev/null 2>&1 || { echo "FAIL: cargo not found." >&2; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "FAIL: jq not found." >&2; exit 1; }

# Build the list of kask library crate -p flags and manifest paths
# (kask/crates/ + kask/mcp-servers/ + zed-path ../crates/hkask-*).
# Overridable via KASK_UNUSED_DEPS_DIRS for check-unused-deps-selftest.sh.
if [ -n "${KASK_UNUSED_DEPS_DIRS+x}" ]; then
    # shellcheck disable=SC2206
    scan_dirs=( $KASK_UNUSED_DEPS_DIRS )
else
    scan_dirs=( crates/hkask-* crates/kask_bridge mcp-servers/hkask-mcp-* ../crates/hkask-* ../crates/hkask-panel-ui )
fi

kask_crates=()
kask_manifests=()
first_manifest=""
for dir in "${scan_dirs[@]}"; do
    if [ -f "$dir/Cargo.toml" ]; then
        name=$(awk '/^\[package\]/{p=1;next} /^\[/{p=0} p && /^name[[:space:]]*=/{sub(/^name[[:space:]]*=[[:space:]]*"/, ""); sub(/".*$/, ""); print; exit}' "$dir/Cargo.toml")
        if [ -n "$name" ]; then
            kask_crates+=("-p" "$name")
            abs_manifest="$(realpath "$dir/Cargo.toml")"
            kask_manifests+=("$abs_manifest")
            [ -n "$first_manifest" ] || first_manifest="$abs_manifest"
        fi
    fi
done

if [ "${#kask_crates[@]}" -eq 0 ]; then
    echo "FAIL: zero kask crates discovered — refusing to pass vacuously." >&2
    exit 1
fi

TMP_JSON=$(mktemp)
TMP_ERR=$(mktemp)
MANIFESTS_JSON=$(mktemp)
trap 'rm -f "$TMP_JSON" "$TMP_ERR" "$MANIFESTS_JSON"' EXIT

printf '%s\n' "${kask_manifests[@]}" | jq -R . | jq -s . > "$MANIFESTS_JSON"

manifest_args=()
if [ -n "${KASK_UNUSED_DEPS_MANIFEST_PATH:-}" ]; then
    manifest_args=("--manifest-path" "$KASK_UNUSED_DEPS_MANIFEST_PATH")
fi

# Use `-W unused_crate_dependencies` with `--message-format=json` on the pinned
# workspace toolchain (no unpinned `rustup toolchain install nightly`, and no
# early abort on transitive workspace deps like `perf`), while failing closed
# if `cargo check` itself exits non-zero on a compile error.
set +e
RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-W unused_crate_dependencies" \
    cargo check --locked --lib --message-format=json "${manifest_args[@]}" "${kask_crates[@]}" >"$TMP_JSON" 2>"$TMP_ERR"
cargo_rc=$?
set -e

if [ "$cargo_rc" -ne 0 ]; then
    echo "FAIL: cargo check --locked --lib failed (exit $cargo_rc):" >&2
    cat "$TMP_ERR" >&2
    jq -r 'select(.reason == "compiler-message" and .message.level == "error") | .message.rendered' "$TMP_JSON" >&2 || true
    exit 1
fi

errors=$(jq -r --slurpfile allowed "$MANIFESTS_JSON" '
    select(.reason == "compiler-message" and .message.code.code == "unused_crate_dependencies")
    | select(.manifest_path as $m | ($allowed[0] | index($m)) != null)
    | .message.rendered
' "$TMP_JSON")

if [ -z "$errors" ]; then
    echo "OK: No unused crate dependencies in kask crates (${#kask_manifests[@]} checked)."
else
    echo ""
    echo "FAIL: Unused crate dependencies found:"
    echo "$errors"
    echo ""
    echo "Remove the unused dependencies from the crate's Cargo.toml,"
    echo "move test-only deps to [dev-dependencies], or add"
    echo "#![allow(unused_crate_dependencies)] to the lib root with a"
    echo "comment explaining why (e.g. bin-needs-dep case)."
    exit 1
fi
