#!/usr/bin/env bash
# Validate inventory structure/references only; never execute registry commands.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
REGISTRY="${1:-$ROOT/kask/docs/architecture/principle-constraints.yaml}"
exec cargo run --locked --quiet --manifest-path "$ROOT/Cargo.toml" \
  -p hkask-regulation --bin check_principle_constraints -- "$ROOT" "$REGISTRY"
