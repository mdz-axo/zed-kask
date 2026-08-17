#!/usr/bin/env bash
# CI gate: enforce RR-0020 — every hkask-* library crate must declare an
# unsafe-gating attribute on line 1 of its crate root file.
#
# Crate roots are discovered from [lib] path in Cargo.toml, falling back to
# src/lib.rs. Kask crates use non-standard names (hkask_types.rs, etc.)
# per the project's [lib] path convention.
#
# Accepted attributes (line 1):
#   #![forbid(unsafe_code)]                          — zero unsafe, no overrides
#   #![cfg_attr(not(test), forbid(unsafe_code))]     — test-only unsafe
#   #![deny(unsafe_code)]                            — production unsafe (with scoped #[allow])
#
# Exit codes:
#   0 — all crate root files have an unsafe-gating attribute
#   1 — one or more crate root files are missing the attribute

set -euo pipefail
cd "$(dirname "$0")/.."

# Overridable via env var so the self-test can point at a temp tree; the
# default preserves the production behavior exactly. SCAN_DIRS is an array
# of glob patterns expanded unquoted in the for loop.
if [ -n "${SCAN_DIRS+x}" ]; then
  # shellcheck disable=SC2206
  scan_dirs=($SCAN_DIRS)
else
  scan_dirs=(
    crates/*/ mcp-servers/*/
    ../crates/swarm_panel/ ../crates/hkask-viz-core/
    ../crates/kask_extensions_ui/ ../crates/marketplace_ui_common/
    ../crates/hkask-scenarios-widget/ ../crates/hkask-portfolio-widget/
    ../crates/hkask-kanban-widget/
  )
fi

violations=0
checked=0

for dir in "${scan_dirs[@]}"; do
  [ -f "$dir/Cargo.toml" ] || continue
  # Extract [lib] path from Cargo.toml (falls back to src/lib.rs).
  lib_path=$(grep -A2 '^\[lib\]' "$dir/Cargo.toml" 2>/dev/null \
    | grep 'path' \
    | sed 's/.*path\s*=\s*"\(.*\)"/\1/' \
    | head -1)
  [ -z "$lib_path" ] && lib_path="src/lib.rs"
  root="$dir$lib_path"
  [ -f "$root" ] || continue
  checked=$((checked + 1))
  first_line=$(head -1 "$root")
  if echo "$first_line" | grep -q 'forbid(unsafe_code)\|deny(unsafe_code)'; then
    : # OK — has an unsafe-gating attribute
  else
    echo "::error::RR-0020: $root is missing an unsafe-gating attribute on line 1"
    echo "  current line 1: $first_line"
    violations=$((violations + 1))
  fi
done

if [ "$violations" -eq 0 ]; then
  echo "OK: $checked crate root files checked, all have unsafe-gating attributes."
else
  echo "summary: $violations violation(s) out of $checked crate root files"
fi

[ "$violations" -eq 0 ]