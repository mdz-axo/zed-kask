#!/usr/bin/env bash
# Run a command with every kask path on a temp dir and no operator credentials or stores.
# Keeps PATH, HOME (cargo/rustup need it) and nothing kask-specific. Usage: iso.sh <command> [args...]
set -uo pipefail
t="$(mktemp -d)"; trap 'rm -rf "$t"' EXIT
drop=(); while IFS= read -r v; do drop+=(-u "$v"); done < <(env | cut -d= -f1 | grep -E '^(HKASK_|KASK_)')
env "${drop[@]}" XDG_RUNTIME_DIR="$t/run" XDG_CONFIG_HOME="$t/config" XDG_DATA_HOME="$t/xdg-data" \
  HKASK_DATA_DIR="$t/data" HKASK_ARTIFACTS_DIR="$t/artifacts" HKASK_EXPERIMENTATION_DB="$t/experimentation.db" \
  HKASK_DB_INVENTORY_PATH="$t/inventory.jsonl" HKASK_DB_PASSPHRASE=mcp-tool-review-probe-only "$@"
