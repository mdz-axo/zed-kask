#!/usr/bin/env bash
# CI gate: enforce RR-0020 — every hkask-* library crate must declare an
# unsafe-gating attribute on line 1 of src/lib.rs.
#
# Accepted attributes (line 1):
#   #![forbid(unsafe_code)]                          — zero unsafe, no overrides
#   #![cfg_attr(not(test), forbid(unsafe_code))]     — test-only unsafe
#   #![deny(unsafe_code)]                            — production unsafe (with scoped #[allow])
#
# Exit codes:
#   0 — all lib.rs files have an unsafe-gating attribute
#   1 — one or more lib.rs files are missing the attribute

set -euo pipefail
cd "$(dirname "$0")/.."

violations=0
checked=0

for f in $(find crates mcp-servers -name lib.rs 2>/dev/null | sort); do
  checked=$((checked + 1))
  first_line=$(head -1 "$f")
  if echo "$first_line" | grep -q 'forbid(unsafe_code)\|deny(unsafe_code)'; then
    : # OK — has an unsafe-gating attribute
  else
    echo "::error::RR-0020: $f is missing an unsafe-gating attribute on line 1"
    echo "  current line 1: $first_line"
    violations=$((violations + 1))
  fi
done

if [ "$violations" -eq 0 ]; then
  echo "OK: $checked lib.rs files checked, all have unsafe-gating attributes."
else
  echo "summary: $violations violation(s) out of $checked lib.rs files"
fi

[ "$violations" -eq 0 ]